//! `check_remote`: upstream releases newer than the lock.
#![cfg(unix)] // maintainer tool; git and path handling are only exercised on macOS and Linux

mod common;

use common::*;
use xtask::upstream::check_remote;
use xtask::upstream::lock::Lock;

#[test]
fn newer_releases_come_with_flagged_changelog_entries() {
    let f = fixture();
    let report = check_remote(&f.ctx()).unwrap();
    assert_eq!(report.newer, ["v0.2.0"]);
    assert!(report.tag_moved.is_none());
    assert!(report.is_behind());
    let got: Vec<(&str, &str)> = report
        .entries
        .iter()
        .map(|(fl, l)| (fl.marker(), l.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (" !", "- **Breaking:** `SolveConfig` gains `x`."),
            (
                "!!",
                "- `PatternEntry` layout changed; regenerate databases."
            ),
            ("  ", "- Docs."),
        ]
    );
}

#[test]
fn the_latest_release_is_up_to_date() {
    let f = fixture();
    let lock = Lock {
        tag: "v0.2.0".into(),
        commit: git(&f.upstream, &["rev-parse", "v0.2.0"]),
        ..f.ctx().lock
    };
    lock.write(&f.ctx().lock_path()).unwrap();
    let report = check_remote(&f.ctx()).unwrap();
    assert!(report.newer.is_empty());
    assert!(!report.is_behind(), "{report:?}");
}

#[test]
fn a_moved_tag_is_reported() {
    let f = fixture();
    git(
        &f.upstream,
        &["tag", "-f", "-a", "v0.1.0", "-m", "moved", "v0.2.0"],
    );
    let report = check_remote(&f.ctx()).unwrap();
    assert!(
        report
            .tag_moved
            .as_deref()
            .unwrap()
            .starts_with("now points at"),
        "{report:?}"
    );
}

#[test]
fn unreachable_upstream_is_an_error() {
    let f = fixture();
    let mut ctx = f.ctx();
    ctx.repo_url = f.root.join("nowhere").to_str().unwrap().to_string();
    assert!(check_remote(&ctx).is_err());
}
