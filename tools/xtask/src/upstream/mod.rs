//! Vendored upstreams as patch queues: `third_party/<name>` is always the locked upstream
//! commit, filtered to the lock's `include` paths, with the patches in
//! `third_party/<name>-patches/series` applied. See docs/upstream.md.
pub mod changelog;
mod check;
mod fetch;
pub mod git;
pub mod lock;
mod queue;
mod remote;
mod sync;
pub mod tags;
pub mod tree;

pub use check::{check_local, LocalReport};
pub use queue::{edit, export, BRANCH};
pub use remote::{check_remote, RemoteReport};
pub use sync::{sync, sync_abort, sync_continue, SyncOutcome};

use anyhow::{Context, Result};
use lock::Lock;
use std::path::{Path, PathBuf};

/// One vendored upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// Directory under `third_party/` and the `--name` value
    pub name: &'static str,
    /// Environment variable naming a mirror to fetch from instead of the lock's `repo`
    pub repo_env: &'static str,
    /// Top-level entries of the vendored tree that are ours, not upstream's
    pub keep: &'static [&'static str],
    /// Packages a sync tests before the workspace
    pub test_packages: &'static [&'static str],
    /// The question a sync's `!!` changelog entries raise
    pub review_hint: &'static str,
}

pub static UPSTREAMS: [Spec; 2] = [
    Spec {
        name: "tetra3",
        repo_env: "UNISOLVER_TETRA3_REPO",
        keep: &["data"],
        test_packages: &["tetra3"],
        review_hint: "does the packed entry layout (PACKED_ENTRY_BYTES) or the database format version change, and must the tiers be re-encoded?",
    },
    Spec {
        name: "seiza",
        repo_env: "UNISOLVER_SEIZA_REPO",
        keep: &[],
        test_packages: &["seiza"],
        review_hint: "does the blind index schema (INDEX_TIER_SCHEMA) or the star tile format change, so the narrow package must be rebuilt?",
    },
];

/// The upstream called `name`.
pub fn spec(name: &str) -> Result<&'static Spec> {
    UPSTREAMS.iter().find(|s| s.name == name).with_context(|| {
        let names: Vec<&str> = UPSTREAMS.iter().map(|s| s.name).collect();
        format!("unknown upstream {name:?} (known: {})", names.join(", "))
    })
}

/// Paths and settings shared by the upstream commands.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// Root of the unisolver checkout.
    pub root: PathBuf,
    pub spec: &'static Spec,
    pub lock: Lock,
    /// Where upstream is fetched from.
    pub repo_url: String,
}

impl Ctx {
    /// Reads the lock of `spec` under `root`. `repo_override` (the `spec.repo_env` value)
    /// replaces its `repo`.
    pub fn load(root: &Path, spec: &'static Spec, repo_override: Option<String>) -> Result<Ctx> {
        let root = std::path::absolute(root)?;
        let lock = Lock::read(&root.join("third_party").join(format!("{}.lock", spec.name)))?;
        let repo_url = repo_override
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| lock.repo.clone());
        Ok(Ctx {
            root,
            spec,
            lock,
            repo_url,
        })
    }

    pub fn vendored(&self) -> PathBuf {
        self.root.join("third_party").join(self.spec.name)
    }

    pub fn patches_dir(&self) -> PathBuf {
        self.root
            .join("third_party")
            .join(format!("{}-patches", self.spec.name))
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root
            .join("third_party")
            .join(format!("{}.lock", self.spec.name))
    }

    /// Untracked working area for the clones and the check's rebuilt tree.
    pub fn scratch(&self) -> PathBuf {
        self.root.join("target/upstream").join(self.spec.name)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lock() -> Lock {
        Lock {
            repo: "https://example.invalid/up".into(),
            tag: "v1.0.0".into(),
            commit: "0123456789abcdef0123456789abcdef01234567".into(),
            include: vec!["src".into()],
        }
    }

    #[test]
    fn every_upstream_has_its_own_paths() {
        let seiza = spec("seiza").unwrap();
        let ctx = Ctx {
            root: PathBuf::from("/r"),
            spec: seiza,
            lock: sample_lock(),
            repo_url: String::new(),
        };
        assert_eq!(ctx.vendored(), PathBuf::from("/r/third_party/seiza"));
        assert_eq!(
            ctx.patches_dir(),
            PathBuf::from("/r/third_party/seiza-patches")
        );
        assert_eq!(ctx.lock_path(), PathBuf::from("/r/third_party/seiza.lock"));
        assert_eq!(
            ctx.cache_dir(),
            PathBuf::from("/r/target/upstream/seiza/cache")
        );
        assert_eq!(spec("tetra3").unwrap().keep, ["data"]);
        assert!(spec("nope").is_err());
    }
}
