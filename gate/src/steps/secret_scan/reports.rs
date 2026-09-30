//! Decode raw findings in memory and emit only locations, hashes and review metadata.

use super::policy::{Exception, fields};
use crate::checks::digests::sha256_hex;
use crate::runner::{Cmd, Failure, Job, Outcome, write};
use std::path::{Component, Path};

/// Minimal finding data; the matched text is discarded immediately after hashing.
pub(super) struct Finding {
    /// Repository-relative location.
    pub(super) path: String,
    /// Gitleaks rule identifier.
    pub(super) rule: String,
    /// First source line.
    pub(super) start: String,
    /// Last source line.
    end: String,
    /// Hash of the complete Match, not Secret: surrounding flagged code also binds approval.
    pub(super) hash: String,
}

/// Require scanner fields instead of silently defaulting malformed output.
const FINDINGS: &str = "if type != \"array\" then error(\"invalid findings\") else . end \
    | .[] | if (.StartLine | type) == \"number\" and .StartLine > 0 \
      and (.EndLine | type) == \"number\" and .EndLine >= .StartLine \
      then [.File,.RuleID,.Match,(.StartLine | tostring),(.EndLine | tostring)] \
      else error(\"invalid location\") end | .[] \
    | if type == \"string\" and (contains(\"\\u0000\") | not) then . \
      else error(\"invalid finding\") end";

/// Map only locations within the archived revision to exact repository-relative paths.
fn relative(file: &str, source: &Path, relocated: Option<&str>) -> Result<String, Failure> {
    let path = Path::new(file);
    let path = if path.is_absolute() {
        path.strip_prefix(source)
            .map_err(|_| "Invalid secret-scan location")?
    } else {
        path
    };
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("Invalid secret-scan location".into());
    }
    let path = path.to_str().ok_or("Invalid secret-scan location")?;
    if let Some(alias) = relocated {
        if path == alias {
            return Ok(".gitleaksignore".to_owned());
        }
        if let Some(child) = path.strip_prefix(&format!("{alias}/")) {
            return Ok(format!(".gitleaksignore/{child}"));
        }
    }
    Ok(path.to_owned())
}

/// Hash the exact Match bytes before any redaction, without persisting them to disk.
pub(super) fn findings(
    bytes: &[u8],
    source: &Path,
    relocated: Option<&str>,
) -> Result<Vec<Finding>, Failure> {
    let values = fields(FINDINGS, bytes)?;
    let mut findings = Vec::new();
    for row in values.chunks(5) {
        let [file, rule, matched, start, end] = row else {
            return Err("Invalid secret-scan data".into());
        };
        findings.push(Finding {
            path: relative(file, source, relocated)?,
            rule: rule.clone(),
            start: start.clone(),
            end: end.clone(),
            hash: sha256_hex(matched.as_bytes()),
        });
    }
    Ok(findings)
}

/// Serialize framed safe fields; jaq escapes paths and reasons rather than interpolating JSON.
const JSON_REPORT: &str = ". as $values | [range(0; length; 13) as $idx \
    | $values[$idx:$idx+13] as $row \
    | {File:$row[0], RuleID:$row[1], StartLine:($row[2]|tonumber), \
       EndLine:($row[3]|tonumber), SHA256:$row[4], Status:$row[5], \
       Match:\"REDACTED\", Secret:\"REDACTED\", \
       Exception:(if $row[5] == \"excepted\" then \
         {repository:$row[6],path:$row[7],rule:$row[8],sha256:$row[9], \
          reason:$row[10],approved_by:$row[11],approved_on:$row[12]} else null end)}]";

/// Standard external suppressions keep excepted locations visible in code scanning.
const SARIF_REPORT: &str = "{version:\"2.1.0\", \
    \"$schema\":\"https://json.schemastore.org/sarif-2.1.0.json\", \
    runs:[{tool:{driver:{name:\"gitleaks\"}},results:map( \
      {ruleId:.RuleID,level:\"error\",message:{text:(\"Secret scan: \" + .Status \
        + \" (sha256 \" + .SHA256 + \")\")}, \
       locations:[{physicalLocation:{artifactLocation:{uri:.File,uriBaseId:\"%SRCROOT%\"}, \
         region:{startLine:.StartLine,endLine:.EndLine}}}], \
       partialFingerprints:{contentSha256:.SHA256},properties:{exception:.Exception}} \
      + (if .Status == \"excepted\" then \
        {suppressions:[{kind:\"external\",status:\"accepted\", \
          justification:.Exception.reason}]} else {} end))}]}";

/// Produce both reports even when unreviewed findings will fail the step.
pub(super) fn reports(
    job: &Job,
    findings: &[Finding],
    approvals: &[Option<&Exception>],
    sarif: bool,
) -> Outcome {
    let mut safe = Vec::new();
    for (finding, approval) in findings.iter().zip(approvals) {
        let status = if approval.is_some() {
            "excepted"
        } else {
            "finding"
        };
        let mut row = vec![
            finding.path.as_str(),
            finding.rule.as_str(),
            finding.start.as_str(),
            finding.end.as_str(),
            finding.hash.as_str(),
            status,
        ];
        if let Some(entry) = approval {
            row.extend([
                entry.repository.as_str(),
                entry.path.as_str(),
                entry.rule.as_str(),
                entry.sha256.as_str(),
                entry.reason.as_str(),
                entry.approved_by.as_str(),
                entry.approved_on.as_str(),
            ]);
        } else {
            row.extend([""; 7]);
        }
        for value in row {
            safe.extend_from_slice(value.as_bytes());
            safe.push(0);
        }
    }
    let json = Cmd::new("jaq --raw-input0 --slurp")
        .arg(JSON_REPORT)
        .stdin_bytes(&safe)
        .capture_bytes()?;
    write(&job.report("secrets.json")?, &json, false)?;
    if sarif {
        Cmd::new("jaq")
            .arg(SARIF_REPORT)
            .stdin_bytes(&json)
            .stdout_to(&job.report("secrets.sarif")?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::relative;
    use std::env;

    #[test]
    fn scanner_locations_are_confined_to_the_archived_revision() {
        let source = env::temp_dir().join("secret-scan-relative");
        let alias = ".rust-gate-gitleaksignore-0";
        assert_eq!(
            relative(alias, &source, Some(alias)).unwrap(),
            ".gitleaksignore"
        );
        assert_eq!(
            relative(&format!("{alias}/file.rs"), &source, Some(alias)).unwrap(),
            ".gitleaksignore/file.rs"
        );
        assert_eq!(
            relative(&format!("{alias}other"), &source, Some(alias)).unwrap(),
            format!("{alias}other")
        );
        assert_eq!(
            relative("src/file.rs", &source, None).unwrap(),
            "src/file.rs"
        );
        assert_eq!(
            relative(source.join("file.rs").to_str().unwrap(), &source, None).unwrap(),
            "file.rs"
        );
        assert!(relative("../file.rs", &source, None).is_err());
        assert!(
            relative(
                source.parent().unwrap().join("file.rs").to_str().unwrap(),
                &source,
                None
            )
            .is_err()
        );
    }
}
