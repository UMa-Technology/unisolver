//! `check_local`: the vendored tree against the locked tag plus the queue.
#![cfg(unix)] // maintainer tool; git and path handling are only exercised on macOS and Linux

mod common;

use common::*;
use xtask::upstream::check_local;
use xtask::upstream::tree::Change;

#[test]
fn clean_tree_passes() {
    let f = fixture();
    let report = check_local(&f.ctx()).unwrap();
    assert!(report.is_clean(), "{report:?}");
}

#[test]
fn any_changed_byte_extra_or_missing_file_is_drift() {
    let f = fixture();
    write(
        &f.vendored(),
        "src/lib.rs",
        "pub fn one() -> u32 {\n    2\n}\n",
    );
    write(&f.vendored(), "src/extra.rs", "");
    std::fs::remove_file(f.vendored().join("LICENSE")).unwrap();
    let report = check_local(&f.ctx()).unwrap();
    assert_eq!(
        report.changes,
        [
            Change::Missing("LICENSE".into()),
            Change::Extra("src/extra.rs".into()),
            Change::Modified("src/lib.rs".into()),
        ]
    );
}

#[test]
fn series_and_patch_files_must_agree() {
    let f = fixture();
    write(&f.patches(), "series", "0001-gone.patch\n");
    write(&f.patches(), "0002-stray.patch", "");
    let report = check_local(&f.ctx()).unwrap();
    assert_eq!(
        report.problems,
        [
            "series lists 0001-gone.patch, which does not exist",
            "0002-stray.patch is not listed in series",
        ]
    );
}

#[test]
fn unreachable_upstream_without_a_cache_is_an_error() {
    let f = fixture();
    let mut ctx = f.ctx();
    ctx.repo_url = f.root.join("nowhere").to_str().unwrap().to_string();
    let err = check_local(&ctx).unwrap_err();
    assert!(
        format!("{err:#}").contains("cannot fetch upstream"),
        "{err:#}"
    );
}

#[test]
fn a_cached_clone_works_offline() {
    let f = fixture();
    check_local(&f.ctx()).unwrap();
    let mut ctx = f.ctx();
    ctx.repo_url = f.root.join("nowhere").to_str().unwrap().to_string();
    assert!(check_local(&ctx).unwrap().is_clean());
}
