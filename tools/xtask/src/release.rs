//! Release bookkeeping: the version each manifest carries, and the CHANGELOG's release headings
//! (`## YYYY-MM-DD — vX.Y.Z`). See docs/releasing.md.
use crate::upstream::tags;
use anyhow::{bail, ensure, Context, Result};
use std::ops::Range;
use std::path::Path;

pub const CHANGELOG: &str = "CHANGELOG.md";
const UNRELEASED: &str = "## Unreleased";

/// A file that carries the release version, and where the version sits in it.
struct Manifest {
    path: &'static str,
    find: fn(&str) -> Option<Range<usize>>,
}

const MANIFESTS: [Manifest; 5] = [
    Manifest {
        path: "Cargo.toml",
        find: cargo_version,
    },
    Manifest {
        path: "packages/unisolver_flutter/pubspec.yaml",
        find: pubspec_version,
    },
    Manifest {
        path: "packages/unisolver_flutter/ios/unisolver_flutter.podspec",
        find: podspec_version,
    },
    Manifest {
        path: "packages/unisolver_flutter/macos/unisolver_flutter.podspec",
        find: podspec_version,
    },
    Manifest {
        path: "packages/unisolver_flutter/example/pubspec.lock",
        find: example_lock_version,
    },
];

/// Files `prepare` rewrites (plus Cargo.lock, which `cargo update --workspace` refreshes).
pub fn prepared_files() -> Vec<&'static str> {
    let mut files = vec![CHANGELOG, "Cargo.lock"];
    files.extend(MANIFESTS.iter().map(|m| m.path));
    files
}

/// Sets every manifest to `version` and turns the Unreleased entries into release `version`
/// dated `date`. Refuses an empty Unreleased section or a version that already has a heading.
/// Nothing is written unless every file can be updated.
pub fn prepare(root: &Path, version: &str, date: &str) -> Result<()> {
    ensure!(
        tags::version(&format!("v{version}")).is_some(),
        "not a version: {version} (want X.Y.Z)"
    );
    ensure!(is_date(date), "not a date: {date} (want YYYY-MM-DD)");
    let mut edits = vec![(
        CHANGELOG,
        finalize_changelog(&read(root, CHANGELOG)?, version, date)?,
    )];
    for m in &MANIFESTS {
        let text = read(root, m.path)?;
        let at = (m.find)(&text).with_context(|| format!("{}: no version found", m.path))?;
        edits.push((
            m.path,
            format!("{}{version}{}", &text[..at.start], &text[at.end..]),
        ));
    }
    for (path, text) in edits {
        std::fs::write(root.join(path), text).with_context(|| format!("writing {path}"))?;
    }
    Ok(())
}

/// What stands between the tree and tag `tag`: manifests carrying another version, a CHANGELOG
/// whose newest release is not `tag`, entries left under Unreleased. Empty means ready.
pub fn check(root: &Path, tag: &str) -> Result<Vec<String>> {
    let version = tag
        .strip_prefix('v')
        .filter(|_| tags::version(tag).is_some())
        .with_context(|| format!("not a release tag: {tag} (want vX.Y.Z)"))?;
    let mut problems = Vec::new();
    for m in &MANIFESTS {
        let text = read(root, m.path)?;
        let at = (m.find)(&text).with_context(|| format!("{}: no version found", m.path))?;
        if &text[at.clone()] != version {
            problems.push(format!(
                "{}: version {}, expected {version}",
                m.path, &text[at]
            ));
        }
    }
    let text = read(root, CHANGELOG)?;
    match lines(&text).find_map(|(_, _, l)| heading_tag(l)) {
        Some(t) if t == tag => {}
        Some(t) => problems.push(format!(
            "{CHANGELOG}: newest release is {t}, expected {tag}"
        )),
        None => problems.push(format!(
            "{CHANGELOG}: no release heading (run `cargo xtask release prepare {version}`)"
        )),
    }
    if !text[unreleased_body(&text)?].trim().is_empty() {
        problems.push(format!(
            "{CHANGELOG}: entries under `{UNRELEASED}` would miss {tag}"
        ));
    }
    Ok(problems)
}

/// The CHANGELOG section of release `tag`, without its heading: the GitHub release notes.
pub fn notes(root: &Path, tag: &str) -> Result<String> {
    release_notes(&read(root, CHANGELOG)?, tag)
}

fn read(root: &Path, path: &str) -> Result<String> {
    std::fs::read_to_string(root.join(path)).with_context(|| format!("reading {path}"))
}

/// `(start, next line's start, line without its newline)` for each line of `text`.
fn lines(text: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    let mut start = 0;
    text.split_inclusive('\n').map(move |raw| {
        let at = start;
        start += raw.len();
        (at, start, raw.trim_end_matches(['\n', '\r']))
    })
}

/// Byte range of the text between the first two `quote`s of `line`, which starts at `at`.
fn quoted(at: usize, line: &str, quote: char) -> Option<Range<usize>> {
    let open = line.find(quote)? + 1;
    let close = open + line[open..].find(quote)?;
    Some(at + open..at + close)
}

/// `version = "…"` in the `[workspace.package]` table.
fn cargo_version(text: &str) -> Option<Range<usize>> {
    let mut in_table = false;
    for (at, _, line) in lines(text) {
        if line.starts_with('[') {
            in_table = line.trim() == "[workspace.package]";
        } else if in_table {
            if let Some(rest) = line.strip_prefix("version") {
                if rest.trim_start().starts_with('=') {
                    return quoted(at, line, '"');
                }
            }
        }
    }
    None
}

/// Top-level `version: …` of a pubspec.yaml.
fn pubspec_version(text: &str) -> Option<Range<usize>> {
    lines(text).find_map(|(at, _, line)| {
        let rest = line.strip_prefix("version:")?;
        let start = at + line.len() - rest.trim_start().len();
        Some(start..at + line.trim_end().len())
    })
}

/// `s.version = '…'` of a podspec.
fn podspec_version(text: &str) -> Option<Range<usize>> {
    lines(text).find_map(|(at, _, line)| {
        let rest = line.trim_start().strip_prefix("s.version")?;
        if !rest.trim_start().starts_with('=') {
            return None;
        }
        quoted(at, line, '\'')
    })
}

/// `version: "…"` of the `unisolver_flutter` entry in the example app's pubspec.lock.
fn example_lock_version(text: &str) -> Option<Range<usize>> {
    let mut in_entry = false;
    for (at, _, line) in lines(text) {
        if line.starts_with("  ") && !line.starts_with("   ") {
            in_entry = line == "  unisolver_flutter:";
        } else if in_entry && line.trim_start().starts_with("version:") {
            return quoted(at, line, '"');
        }
    }
    None
}

/// The tag of a release heading `## YYYY-MM-DD — vX.Y.Z`; `None` for any other line.
fn heading_tag(line: &str) -> Option<&str> {
    let (_, tag) = line.strip_prefix("## ")?.split_once(" — ")?;
    tags::version(tag).map(|_| tag)
}

/// The body of the Unreleased section: after its heading, up to the next `## ` heading.
fn unreleased_body(text: &str) -> Result<Range<usize>> {
    let mut body = None;
    for (at, next, line) in lines(text) {
        match body {
            None if line == UNRELEASED => body = Some(next),
            Some(start) if line.starts_with("## ") => return Ok(start..at),
            _ => {}
        }
    }
    match body {
        Some(start) => Ok(start..text.len()),
        None => bail!("{CHANGELOG} has no `{UNRELEASED}` heading"),
    }
}

/// Inserts the heading of release `version` under an emptied Unreleased heading.
fn finalize_changelog(text: &str, version: &str, date: &str) -> Result<String> {
    let tag = format!("v{version}");
    ensure!(
        !lines(text).any(|(_, _, l)| heading_tag(l) == Some(tag.as_str())),
        "{CHANGELOG} already has a heading for {tag}"
    );
    let body = unreleased_body(text)?;
    ensure!(
        !text[body.clone()].trim().is_empty(),
        "{CHANGELOG}: `{UNRELEASED}` is empty, nothing to release"
    );
    Ok(format!(
        "{}\n## {date} — {tag}\n{}",
        &text[..body.start],
        &text[body.start..]
    ))
}

fn release_notes(text: &str, tag: &str) -> Result<String> {
    let mut body = None;
    for (at, next, line) in lines(text) {
        match body {
            None if heading_tag(line) == Some(tag) => body = Some(next),
            Some(start) if line.starts_with("## ") => {
                return Ok(format!("{}\n", text[start..at].trim()))
            }
            _ => {}
        }
    }
    match body {
        Some(start) => Ok(format!("{}\n", text[start..].trim())),
        None => bail!("{CHANGELOG} has no release heading for {tag}"),
    }
}

fn is_date(s: &str) -> bool {
    s.len() == 10
        && s.char_indices().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Changelog\n\n## Unreleased\n\n1. **New.** Thing.\n\n\
        ## 2026-09-27 — Repository baseline\n\n1. Old.\n";

    fn at(text: &str, find: fn(&str) -> Option<Range<usize>>) -> &str {
        &text[find(text).expect("found")]
    }

    #[test]
    fn locators_find_each_manifest_format() {
        let cargo = "[workspace]\nmembers = []\n\n[workspace.package]\nversion = \"0.2.0\"\nedition = \"2021\"\n";
        assert_eq!(at(cargo, cargo_version), "0.2.0");
        assert_eq!(cargo_version("[package]\nversion = \"9.9.9\"\n"), None);
        assert_eq!(at("name: x\nversion: 0.2.0\n", pubspec_version), "0.2.0");
        let podspec = "Pod::Spec.new do |s|\n  s.name = 'x'\n  s.version          = '0.2.0'\n";
        assert_eq!(at(podspec, podspec_version), "0.2.0");
        let lock = "packages:\n  other:\n    version: \"1.0.0\"\n  unisolver_flutter:\n    dependency: \"direct main\"\n    source: path\n    version: \"0.2.0\"\n  vector_math:\n    version: \"2.0.0\"\n";
        assert_eq!(at(lock, example_lock_version), "0.2.0");
    }

    #[test]
    fn finalize_dates_the_unreleased_entries() {
        let out = finalize_changelog(LOG, "0.2.0", "2026-09-28").unwrap();
        assert_eq!(
            out,
            "# Changelog\n\n## Unreleased\n\n## 2026-09-28 — v0.2.0\n\n1. **New.** Thing.\n\n\
             ## 2026-09-27 — Repository baseline\n\n1. Old.\n"
        );
        assert_eq!(
            release_notes(&out, "v0.2.0").unwrap(),
            "1. **New.** Thing.\n"
        );
    }

    #[test]
    fn finalize_refuses_an_empty_unreleased_or_a_released_version() {
        let done = finalize_changelog(LOG, "0.2.0", "2026-09-28").unwrap();
        let empty = finalize_changelog(&done, "0.2.1", "2026-09-29").unwrap_err();
        assert!(empty.to_string().contains("is empty"), "{empty}");
        let again = done.replacen("## Unreleased\n", "## Unreleased\n\n1. More.\n", 1);
        let dup = finalize_changelog(&again, "0.2.0", "2026-09-29").unwrap_err();
        assert!(dup.to_string().contains("already has a heading"), "{dup}");
    }

    #[test]
    fn notes_need_the_release_heading() {
        assert!(release_notes(LOG, "v0.2.0").is_err());
    }

    #[test]
    fn dates_are_iso() {
        assert!(is_date("2026-09-28"));
        assert!(!is_date("2026-9-28"));
        assert!(!is_date("28.09.2026"));
    }
}
