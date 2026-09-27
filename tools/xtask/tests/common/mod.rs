//! Hermetic fixtures for the upstream commands: a fake upstream repository with two releases
//! and a fake unisolver checkout that vendors the first, all in a temp dir (no network).
#![allow(dead_code)] // each test file uses a different subset

use std::path::{Path, PathBuf};
use std::process::Command;
use xtask::upstream::{lock::Lock, Ctx};

pub const INCLUDE: [&str; 3] = ["Cargo.toml", "LICENSE", "src"];
pub const CARGO: &str = "[workspace]\nmembers = [\".\"]\n\n[package]\nname = \"demo\"\n";
pub const CARGO_PATCHED: &str = "[package]\nname = \"demo\"\n";
pub const LICENSE: &str = "MIT\n";
pub const LIB_V1: &str = "pub fn one() -> u32 {\n    1\n}\n";
pub const LIB_V2: &str = "pub fn one() -> u32 {\n    1 + 0\n}\n";
pub const CHANGELOG_V1: &str = "# Changelog\n\n## 0.1.0 - 2026-01-01\n\n- First release.\n";
pub const CHANGELOG_V2: &str = "# Changelog\n\n## 0.2.0 - 2026-02-01\n\n\
    - **Breaking:** `SolveConfig` gains `x`.\n\
    - `PatternEntry` layout changed; regenerate databases.\n\
    - Docs.\n\n## 0.1.0 - 2026-01-01\n\n- First release.\n";

/// Runs git in `dir` with a fixed identity and no editor; panics on failure, returns the
/// trimmed stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.autocrlf=false", "-c", "commit.gpgsign=false"])
        .args(["-c", "tag.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .env("GIT_AUTHOR_NAME", "Tester")
        .env("GIT_AUTHOR_EMAIL", "tester@example.com")
        .env("GIT_COMMITTER_NAME", "Tester")
        .env("GIT_COMMITTER_EMAIL", "tester@example.com")
        .env("GIT_EDITOR", "true")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

pub fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

pub struct Fixture {
    _tmp: tempfile::TempDir,
    pub upstream: PathBuf,
    pub root: PathBuf,
}

impl Fixture {
    /// A fresh context: re-reads the lock, which `sync` rewrites.
    pub fn ctx(&self) -> Ctx {
        Ctx::load(&self.root, None).unwrap()
    }

    pub fn vendored(&self) -> PathBuf {
        self.root.join("third_party/tetra3")
    }

    pub fn patches(&self) -> PathBuf {
        self.root.join("third_party/tetra3-patches")
    }
}

/// Upstream `v0.1.0` (annotated tag) and `v0.2.0` (lightweight; rewrites line 2 of
/// `src/lib.rs`, adds changelog entries), and a checkout whose lock pins `v0.1.0` with an
/// empty queue and whose vendored tree is `v0.1.0` filtered to `INCLUDE`, plus `data/`.
pub fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let up = tmp.path().join("upstream");
    std::fs::create_dir_all(&up).unwrap();
    git(&up, &["init", "--quiet"]);
    write(&up, "Cargo.toml", CARGO);
    write(&up, "LICENSE", LICENSE);
    write(&up, "src/lib.rs", LIB_V1);
    write(&up, "CHANGELOG.md", CHANGELOG_V1);
    write(&up, "python/demo.py", "print(1)\n");
    git(&up, &["add", "-A"]);
    git(&up, &["commit", "--quiet", "-m", "release 0.1.0"]);
    git(&up, &["tag", "-a", "v0.1.0", "-m", "v0.1.0"]);
    write(&up, "src/lib.rs", LIB_V2);
    write(&up, "CHANGELOG.md", CHANGELOG_V2);
    git(&up, &["commit", "--quiet", "-am", "release 0.2.0"]);
    git(&up, &["tag", "v0.2.0"]);

    let root = tmp.path().join("repo");
    let lock = Lock {
        repo: up.to_str().unwrap().to_string(),
        tag: "v0.1.0".into(),
        commit: git(&up, &["rev-parse", "v0.1.0^{commit}"]),
        include: INCLUDE.iter().map(|s| s.to_string()).collect(),
    };
    write(&root, "third_party/tetra3.lock", &lock.render());
    write(&root, "third_party/tetra3-patches/series", "");
    write(&root, "third_party/tetra3/Cargo.toml", CARGO);
    write(&root, "third_party/tetra3/LICENSE", LICENSE);
    write(&root, "third_party/tetra3/src/lib.rs", LIB_V1);
    write(&root, "third_party/tetra3/data/cache.bin", "test data");
    Fixture {
        _tmp: tmp,
        upstream: up,
        root,
    }
}

/// Adds one patch the way a maintainer does: edit, change a tracked file in the work clone,
/// commit, export.
pub fn add_patch(f: &Fixture, rel: &str, body: &str, subject: &str) {
    let ctx = f.ctx();
    let work = xtask::upstream::edit(&ctx, false).unwrap();
    write(&work, rel, body);
    git(&work, &["commit", "--quiet", "-am", subject]);
    xtask::upstream::export(&ctx).unwrap();
}
