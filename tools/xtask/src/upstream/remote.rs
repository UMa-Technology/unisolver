//! `check`, remote half: upstream releases newer than the lock.
use super::changelog::{self, Flag};
use super::fetch::{ensure_cache, fetch_tags, has_tag};
use super::git::Git;
use super::{tags, Ctx};
use anyhow::Result;
use std::time::Duration;

const LS_REMOTE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct RemoteReport {
    /// Release tags newer than the lock, oldest first.
    pub newer: Vec<String>,
    /// Set when upstream's locked tag is gone or no longer points at the locked commit.
    pub tag_moved: Option<String>,
    /// Changelog entries from the newest release down to the locked one.
    pub entries: Vec<(Flag, String)>,
    /// Why `entries` is empty although `newer` is not.
    pub note: Option<String>,
}

impl RemoteReport {
    pub fn is_behind(&self) -> bool {
        !self.newer.is_empty() || self.tag_moved.is_some()
    }
}

/// Lists upstream's release tags. An error means upstream could not be reached.
pub fn check_remote(ctx: &Ctx) -> Result<RemoteReport> {
    let out = Git::new(&ctx.root)
        .run_timeout(&["ls-remote", "--tags", &ctx.repo_url], LS_REMOTE_TIMEOUT)?;
    let tags = tags::parse_ls_remote(&out);
    let mut report = RemoteReport {
        newer: tags::newer_than(tags.keys(), &ctx.lock.tag),
        ..RemoteReport::default()
    };
    match tags.get(&ctx.lock.tag) {
        None => report.tag_moved = Some("no longer exists upstream".into()),
        Some(c) if *c != ctx.lock.commit => {
            report.tag_moved = Some(format!(
                "now points at {c}, the lock has {}",
                ctx.lock.commit
            ))
        }
        Some(_) => {}
    }
    if let Some(latest) = report.newer.last() {
        match changelog_at(ctx, latest) {
            Ok(text) => report.entries = changelog::entries_since(&text, &ctx.lock.tag),
            Err(e) => report.note = Some(format!("changelog unavailable: {e:#}")),
        }
    }
    Ok(report)
}

fn changelog_at(ctx: &Ctx, tag: &str) -> Result<String> {
    let git = ensure_cache(ctx)?;
    if !has_tag(&git, tag) {
        fetch_tags(ctx, &git)?;
    }
    git.run(&["show", &format!("refs/tags/{tag}:CHANGELOG.md")])
}
