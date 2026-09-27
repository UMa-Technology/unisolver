//! `edit` and `export`: the queue as commits in the work clone and back.
#![cfg(unix)] // maintainer tool; git and path handling are only exercised on macOS and Linux

mod common;

use common::*;
use xtask::upstream::{check_local, edit, export};

#[test]
fn edit_commit_export_round_trip() {
    let f = fixture();
    let ctx = f.ctx();
    write(&f.vendored(), "stale.txt", "left over");
    let work = edit(&ctx, false).unwrap();
    assert_eq!(work, ctx.work_dir());
    write(&work, "Cargo.toml", CARGO_PATCHED);
    git(
        &work,
        &["commit", "--quiet", "-am", "cargo: drop workspace table"],
    );

    assert_eq!(export(&ctx).unwrap(), 1);
    let name = "0001-cargo-drop-workspace-table.patch";
    assert_eq!(read(&f.patches(), "series"), format!("{name}\n"));
    let patch = read(&f.patches(), name);
    assert!(
        patch.contains("\nFrom: unisolver <patches@unisolver.invalid>\n"),
        "{patch}"
    );
    assert!(!patch.contains("tester@example.com"), "{patch}");
    assert_eq!(read(&f.vendored(), "Cargo.toml"), CARGO_PATCHED);
    assert_eq!(read(&f.vendored(), "data/cache.bin"), "test data");
    assert!(!f.vendored().join("stale.txt").exists());
    assert!(!f.vendored().join("python").exists());
    assert!(check_local(&ctx).unwrap().is_clean());

    // Exporting again is byte-identical
    export(&ctx).unwrap();
    assert_eq!(read(&f.patches(), name), patch);
}

#[test]
fn edit_rebuilds_one_commit_per_patch() {
    let f = fixture();
    add_patch(
        &f,
        "Cargo.toml",
        CARGO_PATCHED,
        "cargo: drop workspace table",
    );
    add_patch(
        &f,
        "src/lib.rs",
        "pub fn one() -> u32 {\n    10\n}\n",
        "lib: ten",
    );
    let ctx = f.ctx();
    let work = edit(&ctx, false).unwrap();
    let range = format!("{}..unisolver", ctx.lock.commit);
    assert_eq!(git(&work, &["rev-list", "--count", &range]), "2");
    assert_eq!(git(&work, &["log", "-1", "--format=%s"]), "lib: ten");
}

#[test]
fn edit_refuses_to_drop_unexported_commits() {
    let f = fixture();
    let ctx = f.ctx();
    let work = edit(&ctx, false).unwrap();
    write(&work, "src/lib.rs", "changed\n");
    git(&work, &["commit", "--quiet", "-am", "wip"]);
    let err = edit(&ctx, false).unwrap_err();
    assert!(format!("{err:#}").contains("never exported"), "{err:#}");
    edit(&ctx, true).unwrap();
    assert_eq!(read(&work, "src/lib.rs"), LIB_V1);
}

#[test]
fn edit_refuses_uncommitted_changes() {
    let f = fixture();
    let ctx = f.ctx();
    let work = edit(&ctx, false).unwrap();
    write(&work, "src/lib.rs", "changed\n");
    let err = edit(&ctx, false).unwrap_err();
    assert!(format!("{err:#}").contains("uncommitted"), "{err:#}");
}

#[test]
fn export_refuses_changes_outside_include() {
    let f = fixture();
    let ctx = f.ctx();
    let work = edit(&ctx, false).unwrap();
    write(&work, "python/demo.py", "print(2)\n");
    git(&work, &["commit", "--quiet", "-am", "python change"]);
    let err = export(&ctx).unwrap_err();
    assert!(format!("{err:#}").contains("python/demo.py"), "{err:#}");
}

#[test]
fn a_patch_that_does_not_apply_is_a_queue_problem() {
    let f = fixture();
    add_patch(
        &f,
        "Cargo.toml",
        CARGO_PATCHED,
        "cargo: drop workspace table",
    );
    write(
        &f.patches(),
        "0001-cargo-drop-workspace-table.patch",
        "not a patch\n",
    );
    let report = check_local(&f.ctx()).unwrap();
    assert_eq!(report.problems.len(), 1, "{report:?}");
    assert!(
        report.problems[0].contains("does not apply to v0.1.0"),
        "{report:?}"
    );
}
