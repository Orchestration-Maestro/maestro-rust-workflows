//! The exception authority is compiled into the gate, never read from a consumer.

use crate::runner::{Cmd, Failure};

/// Reviewed entries shipped with the workflow revision that builds this gate.
const POLICY: &str = include_str!("../../../../policy/secret-scan-exceptions.json");

/// One exact-key approval and its review metadata.
pub(super) struct Exception {
    /// Exact GitHub owner/name.
    pub(super) repository: String,
    /// Exact repository-relative file.
    pub(super) path: String,
    /// Exact Gitleaks rule identifier.
    pub(super) rule: String,
    /// SHA256 of the complete, unredacted Gitleaks Match, including whitespace.
    pub(super) sha256: String,
    /// Why the content is not a credential.
    pub(super) reason: String,
    /// Who approved this entry.
    pub(super) approved_by: String,
    /// Approval date.
    pub(super) approved_on: String,
}

impl Exception {
    /// No normalization or wildcard interpretation can widen an approval.
    pub(super) fn matches(&self, repository: &str, path: &str, rule: &str, hash: &str) -> bool {
        self.repository == repository
            && self.path == path
            && self.rule == rule
            && self.sha256 == hash
    }
}

/// Decode strings with NUL framing; reject NUL content rather than misaligning keys.
pub(super) fn fields(filter: &str, bytes: &[u8]) -> Result<Vec<String>, Failure> {
    let output = Cmd::new("jaq --raw-output0")
        .arg(filter)
        .stdin_bytes(bytes)
        .capture_output()?;
    if !output.status.success() {
        // A parser error may quote input, so never forward its raw diagnostic.
        return Err("Invalid secret-scan data".into());
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "Invalid secret-scan data")?;
    let values: Vec<String> = text.split_terminator('\0').map(str::to_owned).collect();
    if values.iter().any(String::is_empty) {
        return Err("Invalid secret-scan data".into());
    }
    Ok(values)
}

/// Load only the organization-reviewed list embedded at build time.
pub(super) fn exceptions() -> Result<Vec<Exception>, Failure> {
    let values = fields(
        ".[] | [.repository,.path,.rule,.sha256,.reason,.approved_by,.approved_on] | .[] \
         | if type == \"string\" and (contains(\"\\u0000\") | not) then . \
         else error(\"invalid exception\") end",
        POLICY.as_bytes(),
    )?;
    let mut entries = Vec::new();
    for row in values.chunks(7) {
        let [
            repository,
            path,
            rule,
            sha256,
            reason,
            approved_by,
            approved_on,
        ] = row
        else {
            return Err("Invalid secret-scan data".into());
        };
        entries.push(Exception {
            repository: repository.clone(),
            path: path.clone(),
            rule: rule.clone(),
            sha256: sha256.clone(),
            reason: reason.clone(),
            approved_by: approved_by.clone(),
            approved_on: approved_on.clone(),
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::Exception;

    #[test]
    fn approval_requires_every_exact_key_without_wildcards() {
        let entry = Exception {
            repository: "owner/repo".into(),
            path: "vendor/file.rs".into(),
            rule: "rule-id".into(),
            sha256: "hash".into(),
            reason: "reviewed".into(),
            approved_by: "owner".into(),
            approved_on: "2026-09-30".into(),
        };
        for (repository, path, rule, hash, expected) in [
            ("owner/repo", "vendor/file.rs", "rule-id", "hash", true),
            ("other/repo", "vendor/file.rs", "rule-id", "hash", false),
            ("owner/repo", "vendor/other.rs", "rule-id", "hash", false),
            ("owner/repo", "vendor/file.rs", "other-rule", "hash", false),
            (
                "owner/repo",
                "vendor/file.rs",
                "rule-id",
                "other-hash",
                false,
            ),
            ("owner/*", "vendor/file.rs", "rule-id", "hash", false),
        ] {
            assert_eq!(entry.matches(repository, path, rule, hash), expected);
        }
    }
}
