//! `sync`: rebase the queue onto another upstream tag, export it and move the lock.
use super::changelog::{self, Flag};
use super::fetch::{ensure_cache, fetch_tags, has_tag, resolve_tag};
use super::git::Git;
use super::queue::{edit, ensure_idle, export, rebase_in_progress, BRANCH};
use super::Ctx;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum SyncOutcome {
    /// The lock already pins that tag.
    Noop,
    /// Rebased, exported, lock moved; `entries` are upstream's changelog lines in between.
    Synced {
        from: String,
        to: String,
        entries: Vec<(Flag, String)>,
    },
    /// The rebase stopped on a conflict in the work clone.
    Conflict { work: PathBuf },
}

/// A sync between its rebase and its export, kept in `target/upstream/sync.toml`.
#[derive(Serialize, Deserialize)]
struct State {
    from_tag: String,
    from_commit: String,
    to_tag: String,
    to_commit: String,
}

pub fn sync(ctx: &Ctx, tag: &str) -> Result<SyncOutcome> {
    ensure!(
        !ctx.sync_state().exists(),
        "a sync is in progress: finish it with `sync --continue` or give up with `sync --abort`"
    );
    if tag == ctx.lock.tag {
        return Ok(SyncOutcome::Noop);
    }
    edit(ctx, false)?;
    let cache = ensure_cache(ctx)?;
    if !has_tag(&cache, tag) {
        fetch_tags(ctx, &cache)?;
    }
    let state = State {
        from_tag: ctx.lock.tag.clone(),
        from_commit: ctx.lock.commit.clone(),
        to_tag: tag.to_string(),
        to_commit: resolve_tag(&cache, tag)?,
    };
    let work = Git::new(ctx.work_dir());
    work.run(&["fetch", "--quiet", "--tags", "--force", "origin"])?;
    std::fs::write(ctx.sync_state(), toml::to_string(&state)?)?;
    let rebase = [
        "rebase",
        "--quiet",
        "--onto",
        &state.to_commit,
        &state.from_commit,
        BRANCH,
    ];
    if let Err(e) = work.run(&rebase) {
        if rebase_in_progress(&work)? {
            return Ok(SyncOutcome::Conflict {
                work: ctx.work_dir(),
            });
        }
        let _ = std::fs::remove_file(ctx.sync_state());
        return Err(e);
    }
    finish(ctx, &state)
}

/// Finishes a sync whose rebase conflict was resolved and continued in the work clone.
pub fn sync_continue(ctx: &Ctx) -> Result<SyncOutcome> {
    let text = std::fs::read_to_string(ctx.sync_state()).context("no sync in progress")?;
    finish(ctx, &toml::from_str(&text)?)
}

/// Gives up an interrupted sync: aborts the rebase and forgets the state. The lock, the queue
/// and the vendored tree were not touched.
pub fn sync_abort(ctx: &Ctx) -> Result<()> {
    Git::new(ctx.work_dir()).succeeds(&["rebase", "--abort"]);
    let _ = std::fs::remove_file(ctx.sync_state());
    Ok(())
}

fn finish(ctx: &Ctx, state: &State) -> Result<SyncOutcome> {
    let work = Git::new(ctx.work_dir());
    ensure_idle(&work, false)?;
    ensure!(
        work.succeeds(&["merge-base", "--is-ancestor", &state.to_commit, BRANCH]),
        "branch {BRANCH} is not on top of {}",
        state.to_tag
    );
    let mut next = ctx.clone();
    next.lock.tag = state.to_tag.clone();
    next.lock.commit = state.to_commit.clone();
    export(&next)?;
    next.lock.write(&ctx.lock_path())?;
    let text = work
        .run(&["show", &format!("{}:CHANGELOG.md", state.to_commit)])
        .unwrap_or_default();
    std::fs::remove_file(ctx.sync_state())?;
    Ok(SyncOutcome::Synced {
        from: state.from_tag.clone(),
        to: state.to_tag.clone(),
        entries: changelog::entries_since(&text, &state.from_tag),
    })
}
