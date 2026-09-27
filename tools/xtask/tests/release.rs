//! `release`: against the repository's own manifests, and a prepare/check round trip on copies.
use std::path::{Path, PathBuf};
use xtask::release::{check, notes, prepare, prepared_files, CHANGELOG};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// The version of the workspace, as a tag.
fn current_tag() -> String {
    let cargo = std::fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    let table = cargo.split("[workspace.package]").nth(1).unwrap();
    let line = table.lines().find(|l| l.starts_with("version")).unwrap();
    format!("v{}", line.split('"').nth(1).unwrap())
}

/// A temp dir holding copies of every file `prepare` touches except Cargo.lock, with LF line
/// endings (a Windows checkout may have CRLF).
fn copy_of_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for f in prepared_files().into_iter().filter(|f| *f != "Cargo.lock") {
        let to = dir.path().join(f);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        let text = std::fs::read_to_string(repo_root().join(f)).unwrap();
        std::fs::write(to, text.replace("\r\n", "\n")).unwrap();
    }
    dir
}

#[test]
fn every_manifest_in_the_repository_carries_the_workspace_version() {
    let tag = current_tag();
    let problems = check(&repo_root(), &tag).unwrap();
    let manifests: Vec<&String> = problems
        .iter()
        .filter(|p| !p.starts_with(CHANGELOG))
        .collect();
    assert!(manifests.is_empty(), "{manifests:?}");
}

#[test]
fn prepare_then_check_passes_and_a_second_prepare_is_refused() {
    let dir = copy_of_repo();
    let root = dir.path();
    let log = std::fs::read_to_string(root.join(CHANGELOG)).unwrap();
    let log = log.replacen(
        "## Unreleased\n",
        "## Unreleased\n\n1. **Test entry.** Only here.\n",
        1,
    );
    std::fs::write(root.join(CHANGELOG), log).unwrap();

    prepare(root, "9.8.7", "2031-02-03").unwrap();
    assert_eq!(check(root, "v9.8.7").unwrap(), Vec::<String>::new());
    let log = std::fs::read_to_string(root.join(CHANGELOG)).unwrap();
    assert!(
        log.contains("## Unreleased\n\n## 2031-02-03 — v9.8.7\n"),
        "{log}"
    );
    assert!(notes(root, "v9.8.7")
        .unwrap()
        .contains("**Test entry.** Only here."));

    let err = prepare(root, "9.8.8", "2031-02-04").unwrap_err();
    assert!(err.to_string().contains("is empty"), "{err}");
}

#[test]
fn check_names_every_mismatch() {
    let dir = copy_of_repo();
    let root = dir.path();
    let pubspec = "packages/unisolver_flutter/pubspec.yaml";
    let text = std::fs::read_to_string(root.join(pubspec)).unwrap();
    let text = text.replacen(
        &format!("version: {}", &current_tag()[1..]),
        "version: 0.0.9",
        1,
    );
    std::fs::write(root.join(pubspec), text).unwrap();
    std::fs::write(
        root.join(CHANGELOG),
        "# Changelog\n\n## Unreleased\n\n1. Pending.\n",
    )
    .unwrap();

    let problems = check(root, &current_tag()).unwrap();
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems[0].starts_with(pubspec) && problems[0].contains("version 0.0.9"));
    assert!(problems[1].contains("no release heading"));
    assert!(problems[2].contains("would miss"));
}

#[test]
fn a_bad_tag_is_an_error() {
    assert!(check(&repo_root(), "0.2.0").is_err());
    assert!(check(&repo_root(), "v0.2").is_err());
}
