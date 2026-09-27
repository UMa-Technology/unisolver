//! Upstream CHANGELOG.md entries newer than the pinned release, flagged where they reach the
//! patch surface. Release headings look like `## 0.13.0 - 2026-09-04`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    /// Touches the patch surface or asks for regenerated databases.
    Patch,
    /// A new or changed config field, or a breaking change.
    Config,
    None,
}

impl Flag {
    pub fn marker(self) -> &'static str {
        match self {
            Flag::Patch => "!!",
            Flag::Config => " !",
            Flag::None => "  ",
        }
    }
}

// Patch 0003 replaces the pattern-table container and hooks database load/save
const PATCH_TERMS: &[&str] = &[
    "PatternEntry",
    "PatternCatalog",
    "pattern_catalog",
    "save_to_file",
    "load_from_file",
    "to_bytes",
    "from_bytes",
    "validate",
    "regenerat",
    "Regenerat",
];
const CONFIG_TERMS: &[&str] = &["SolveConfig", "Breaking", "breaking"];

pub fn flag(line: &str) -> Flag {
    if PATCH_TERMS.iter().any(|t| line.contains(t)) {
        Flag::Patch
    } else if CONFIG_TERMS.iter().any(|t| line.contains(t)) {
        Flag::Config
    } else {
        Flag::None
    }
}

/// Entry lines (`- ...` and `**...`) from the top of the changelog down to the heading of
/// `pinned` (a tag such as `v0.13.0`), each with its flag.
pub fn entries_since(text: &str, pinned: &str) -> Vec<(Flag, String)> {
    let pinned = pinned.trim_start_matches('v');
    let mut in_release = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if heading.split_whitespace().next() == Some(pinned) {
                break;
            }
            in_release = true;
        } else if in_release && (line.starts_with("- ") || line.starts_with("**")) {
            out.push((flag(line), line.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\nIntro.\n\n## 0.14.0 - 2026-10-01\n\n\
        **Upgrading:** regenerate databases.\n\n### Added\n\n\
        - `SolveConfig::foo` (default 3).\n- Faster centroiding.\n- `PatternEntry` gains a field.\n\n\
        ## 0.13.0 - 2026-09-04\n\n- Old entry.\n";

    #[test]
    fn entries_stop_at_the_pinned_release_and_carry_flags() {
        let entries = entries_since(LOG, "v0.13.0");
        let got: Vec<(&str, &str)> = entries
            .iter()
            .map(|(f, l)| (f.marker(), l.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("!!", "**Upgrading:** regenerate databases."),
                (" !", "- `SolveConfig::foo` (default 3)."),
                ("  ", "- Faster centroiding."),
                ("!!", "- `PatternEntry` gains a field."),
            ]
        );
    }

    #[test]
    fn nothing_when_the_pinned_release_is_on_top() {
        assert!(entries_since(LOG, "v0.14.0").is_empty());
    }
}
