//! The scorecard of one run as a value, and the three renderings the step
//! writes from it. An internal seam of the step beside it, reaching no
//! further than its parent: the JSON a machine reads, the Markdown the log and the
//! step summary show, and the self-contained badge.

use std::fmt::Write as _;

/// What the run proved about one control, not merely whether its step was green.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    /// Selected, applicable and successful.
    Passed,
    /// The caller switched the control off.
    Disabled,
    /// The step explicitly established there was nothing applicable to check.
    NotApplicable,
    /// The selected control failed.
    Failed,
    /// No completed execution or applicability evidence was available.
    NotRun,
}

impl State {
    /// Combine GitHub's outcome with selection and the step's application result.
    pub(super) fn from_outcome(outcome: &str, enabled: bool, applied: &str) -> Self {
        if !enabled {
            return Self::Disabled;
        }
        match (outcome, applied) {
            ("failure", _) => Self::Failed,
            ("success", "true") => Self::Passed,
            ("success" | "skipped", "false") => Self::NotApplicable,
            _ => Self::NotRun,
        }
    }

    /// Parse the stable state spelling shared by JSON and Markdown.
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "passed" => Some(Self::Passed),
            "disabled" => Some(Self::Disabled),
            "not-applicable" => Some(Self::NotApplicable),
            "failed" => Some(Self::Failed),
            "not-run" => Some(Self::NotRun),
            _ => None,
        }
    }

    /// Stable spellings shared by JSON and Markdown.
    fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Disabled => "disabled",
            Self::NotApplicable => "not-applicable",
            Self::Failed => "failed",
            Self::NotRun => "not-run",
        }
    }
}

/// One control's name, `enforced` or `optional` classification, and proven state.
pub(super) type Control = (&'static str, &'static str, State);

/// An owned row read back from the preliminary JSON scorecard.
pub(super) type OwnedControl = (String, String, State);

/// One run's scorecard: the controls it gathered and the facts every
/// rendering of them repeats.
pub(super) struct Scorecard {
    /// Every control, in the order the table lists them.
    pub(super) controls: Vec<OwnedControl>,
    /// The revision this run checked.
    pub(super) revision: String,
    /// The compiler this run used.
    pub(super) toolchain: String,
    /// Line coverage, when the coverage gate left a report.
    pub(super) coverage: Option<String>,
    /// Optional complexity data, when that step produced a report.
    pub(super) complexity: Option<String>,
}

impl Scorecard {
    /// How many of the controls this run actually enforced.
    fn active(&self) -> usize {
        self.controls
            .iter()
            .filter(|(_, _, state)| *state == State::Passed)
            .count()
    }

    /// The scorecard as one JSON document, one line, for machines.
    pub(super) fn json(&self) -> String {
        let rows: Vec<String> = self
            .controls
            .iter()
            .map(|(control, kind, state)| {
                format!(
                    "{{\"control\":\"{control}\",\"kind\":\"{kind}\",\
                     \"active\":{},\"state\":\"{}\"}}",
                    *state == State::Passed,
                    state.as_str()
                )
            })
            .collect();
        let (revision, toolchain) = (&self.revision, &self.toolchain);
        let complexity = self
            .complexity
            .as_deref()
            .map(|data| format!(",\"complexity\":{data}"))
            .unwrap_or_default();
        format!(
            "{{\"revision\":\"{revision}\",\"toolchain\":\"{toolchain}\",\"coverage\":{},\
             \"controls\":[{}],\"active\":{},\"available\":{}{complexity}}}\n",
            self.coverage.as_deref().unwrap_or("null"),
            rows.join(","),
            self.active(),
            self.controls.len()
        )
    }

    /// The scorecard as the Markdown the log and the step summary show.
    pub(super) fn markdown(&self) -> String {
        let (revision, toolchain) = (&self.revision, &self.toolchain);
        let mut markdown = format!(
            "## Rust quality scorecard\n\n`{revision}` on Rust {toolchain}\n\n\
             | Control | Kind | State |\n| --- | --- | --- |\n"
        );
        for (control, kind, state) in &self.controls {
            let _ = writeln!(markdown, "| {control} | {kind} | {} |", state.as_str());
        }
        let coverage_suffix = self
            .coverage
            .as_ref()
            .map(|value| format!(" \u{b7} {value}% line coverage"))
            .unwrap_or_default();
        let _ = write!(
            markdown,
            "\n**{}/{} controls active{coverage_suffix}**\n",
            self.active(),
            self.controls.len()
        );
        markdown
    }

    /// Self-contained badge: no external service, no runtime dependency, and
    /// the palette is sampled from this repository's own banner. Geometry is
    /// authored at 10x and scaled down so the text stays crisp, so every
    /// coordinate inside the scaled group is ten times its rendered value. The
    /// value segment turns red when an enforced control did not pass.
    pub(super) fn badge(&self) -> String {
        let label = "rust quality";
        let value = format!(
            "{}/{}{}",
            self.active(),
            self.controls.len(),
            self.coverage
                .as_ref()
                .map(|value| format!(" \u{b7} {value}%"))
                .unwrap_or_default()
        );
        let accent = if self.controls.iter().any(|(_, kind, state)| {
            *state == State::Failed || (*kind == "enforced" && *state == State::NotRun)
        }) {
            "#8c3a2b"
        } else {
            "#3f7d3f"
        };
        let label_width = label.chars().count() * 62 / 10 + 20;
        let value_width = value.chars().count() * 62 / 10 + 20;
        let width = label_width + value_width;
        // Glyph run widths in tenths of a pixel: the segment minus its 10px
        // padding on each side.
        let label_text_width = (label_width - 20) * 10;
        let value_text_width = (value_width - 20) * 10;
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="20"
         viewBox="0 0 {width} 20" role="img" aria-label="{label}: {value}">
      <title>{label}: {value}</title>
      <linearGradient id="rqsc-sheen" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="#ffffff" stop-opacity=".13"/>
        <stop offset="1" stop-color="#000000" stop-opacity=".2"/>
      </linearGradient>
      <clipPath id="rqsc-clip"><rect width="{width}" height="20" rx="4"/></clipPath>
      <g clip-path="url(#rqsc-clip)">
        <rect width="{label_width}" height="20" fill="#141109"/>
        <rect x="{label_width}" width="{value_width}" height="20" fill="{accent}"/>
        <rect x="{label_width}" width="1" height="20" fill="#040800"/>
        <rect width="{width}" height="20" fill="url(#rqsc-sheen)"/>
        <rect width="{width}" height="20" rx="4" fill="none"
              stroke="#886c48" stroke-opacity=".5" stroke-width="2"/>
      </g>
      <g font-family="Verdana,DejaVu Sans,Geneva,sans-serif" font-size="110"
         text-rendering="geometricPrecision">
        <g transform="scale(.1)">
          <text x="100" y="150" fill="#040800" fill-opacity=".45"
                textLength="{label_text_width}" lengthAdjust="spacing">{label}</text>
          <text x="100" y="140" fill="#bab79c"
                textLength="{label_text_width}" lengthAdjust="spacing">{label}</text>
        </g>
        <g transform="translate({label_width},0) scale(.1)">
          <text x="100" y="150" fill="#040800" fill-opacity=".45"
                textLength="{value_text_width}" lengthAdjust="spacing">{value}</text>
          <text x="100" y="140" fill="#ffffff"
                textLength="{value_text_width}" lengthAdjust="spacing">{value}</text>
        </g>
      </g>
    </svg>
    "##
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Control, Scorecard, State};

    /// A scorecard of the given controls, the rest of its facts fixed.
    fn scorecard(controls: Vec<Control>, coverage: Option<&str>) -> Scorecard {
        Scorecard {
            controls: controls
                .into_iter()
                .map(|(name, kind, state)| (name.to_owned(), kind.to_owned(), state))
                .collect(),
            revision: "abc".to_owned(),
            toolchain: "1.98.1".to_owned(),
            coverage: coverage.map(str::to_owned),
            complexity: None,
        }
    }

    #[test]
    fn the_badge_turns_red_when_an_enforced_control_failed() {
        let green = scorecard(
            vec![
                ("advisories", "enforced", State::Passed),
                ("mutation testing", "optional", State::Disabled),
            ],
            Some("91.5"),
        )
        .badge();
        assert!(green.contains("#3f7d3f") && green.contains("1/2 \u{b7} 91.5%"));
        let red = scorecard(vec![("advisories", "enforced", State::Failed)], None).badge();
        assert!(red.contains("#8c3a2b") && red.contains(">0/1<"));
        let json = scorecard(vec![("advisories", "enforced", State::Passed)], None).json();
        assert_eq!(
            json,
            concat!(
                "{\"revision\":\"abc\",\"toolchain\":\"1.98.1\",\"coverage\":null,",
                "\"controls\":[{\"control\":\"advisories\",\"kind\":\"enforced\",",
                "\"active\":true,\"state\":\"passed\"}],\"active\":1,\"available\":1}\n"
            )
        );
    }
}
