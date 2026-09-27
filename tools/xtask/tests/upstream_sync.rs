//! `sync`: rebase the queue onto another tag, export, move the lock.
#![cfg(unix)] // maintainer tool; git and path handling are only exercised on macOS and Linux

mod common;

use common::*;
use xtask::upstream::{check_local, edit, sync, sync_abort, sync_continue, SyncOutcome};

const LIB_TEN: &str = "pub fn one() -> u32 {\n    10\n}\n";

#[test]
fn the_locked_tag_is_a_no_op() {
    let f = fixture();
    assert_eq!(sync(&f.ctx(), "v0.1.0").unwrap(), SyncOutcome::Noop);
    assert!(!f.ctx().work_dir().exists());
}

#[test]
fn a_clean_rebase_exports_and_moves_the_lock() {
    let f = fixture();
    add_patch(
        &f,
        "Cargo.toml",
        CARGO_PATCHED,
        "cargo: drop workspace table",
    );
    let SyncOutcome::Synced { from, to, entries } = sync(&f.ctx(), "v0.2.0").unwrap() else {
        panic!("expected Synced");
    };
    assert_eq!((from.as_str(), to.as_str()), ("v0.1.0", "v0.2.0"));
    assert_eq!(entries.len(), 3);
    let ctx = f.ctx();
    assert_eq!(ctx.lock.tag, "v0.2.0");
    assert_eq!(ctx.lock.commit, git(&f.upstream, &["rev-parse", "v0.2.0"]));
    assert_eq!(read(&f.vendored(), "src/lib.rs"), LIB_V2);
    assert_eq!(read(&f.vendored(), "Cargo.toml"), CARGO_PATCHED);
    assert_eq!(
        read(&f.patches(), "series"),
        "0001-cargo-drop-workspace-table.patch\n"
    );
    assert!(check_local(&ctx).unwrap().is_clean());
    assert!(!ctx.sync_state().exists());
}

#[test]
fn a_conflict_stops_and_continue_finishes() {
    let f = fixture();
    add_patch(&f, "src/lib.rs", LIB_TEN, "lib: ten");
    let SyncOutcome::Conflict { work } = sync(&f.ctx(), "v0.2.0").unwrap() else {
        panic!("expected Conflict");
    };
    assert!(f.ctx().sync_state().exists());
    assert!(
        sync_continue(&f.ctx()).is_err(),
        "must refuse while the rebase is stopped"
    );
    let resolved = "pub fn one() -> u32 {\n    10 + 0\n}\n";
    write(&work, "src/lib.rs", resolved);
    git(&work, &["add", "src/lib.rs"]);
    git(&work, &["rebase", "--continue"]);
    assert!(matches!(
        sync_continue(&f.ctx()).unwrap(),
        SyncOutcome::Synced { .. }
    ));
    assert_eq!(read(&f.vendored(), "src/lib.rs"), resolved);
    assert_eq!(f.ctx().lock.tag, "v0.2.0");
    assert!(check_local(&f.ctx()).unwrap().is_clean());
}

#[test]
fn abort_leaves_everything_as_it_was() {
    let f = fixture();
    add_patch(&f, "src/lib.rs", LIB_TEN, "lib: ten");
    assert!(matches!(
        sync(&f.ctx(), "v0.2.0").unwrap(),
        SyncOutcome::Conflict { .. }
    ));
    sync_abort(&f.ctx()).unwrap();
    let ctx = f.ctx();
    assert!(!ctx.sync_state().exists());
    assert_eq!(ctx.lock.tag, "v0.1.0");
    assert_eq!(read(&f.vendored(), "src/lib.rs"), LIB_TEN);
    edit(&ctx, false).unwrap();
}
