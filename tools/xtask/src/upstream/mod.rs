//! The vendored tetra3rs copy as a patch queue: `third_party/tetra3` is always the locked
//! upstream commit, filtered to the lock's `include` paths, with the patches in
//! `third_party/tetra3-patches/series` applied. See docs/upstream.md.
pub mod changelog;
mod check;
mod fetch;
pub mod git;
pub mod lock;
mod queue;
pub mod tags;
pub mod tree;

pub use check::{check_local, LocalReport};
pub use queue::{edit, export, BRANCH};

use anyhow::{Context, Result};
use lock::Lock;
use std::path::{Path, PathBuf};

/// Environment variable naming a mirror to fetch upstream from instead of the lock's `repo`.
pub const REPO_ENV: &str = "UNISOLVER_TETRA3_REPO";
/// The one top-level entry of the vendored tree that is not upstream's: its test-data cache.
pub const VENDORED_KEEP: &str = "data";

/// Paths and settings shared by the upstream commands.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// Root of the unisolver checkout.
    pub root: PathBuf,
    pub lock: Lock,
    /// Where upstream is fetched from.
    pub repo_url: String,
}

impl Ctx {
    /// Reads the lock under `root`. `repo_override` (the `REPO_ENV` value) replaces its `repo`.
    pub fn load(root: &Path, repo_override: Option<String>) -> Result<Ctx> {
        let root = std::path::absolute(root)?;
        let lock = Lock::read(&root.join("third_party/tetra3.lock"))?;
        let repo_url = repo_override
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| lock.repo.clone());
        Ok(Ctx {
            root,
            lock,
            repo_url,
        })
    }

    pub fn vendored(&self) -> PathBuf {
        self.root.join("third_party/tetra3")
    }

    pub fn patches_dir(&self) -> PathBuf {
        self.root.join("third_party/tetra3-patches")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root.join("third_party/tetra3.lock")
    }

    /// Untracked working area for the clones and the check's rebuilt tree.
    pub fn scratch(&self) -> PathBuf {
        self.root.join("target/upstream")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.scratch().join("cache")
    }

    pub fn work_dir(&self) -> PathBuf {
        self.scratch().join("work")
    }

    /// The tree `check` rebuilds from the lock and the queue.
    pub fn expected_dir(&self) -> PathBuf {
        self.scratch().join("expected")
    }

    pub fn sync_state(&self) -> PathBuf {
        self.scratch().join("sync.toml")
    }
}

/// Patch file names from `series`, in order (blank lines and `#` comments skipped).
pub fn read_series(ctx: &Ctx) -> Result<Vec<String>> {
    let path = ctx.patches_dir().join("series");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect())
}
