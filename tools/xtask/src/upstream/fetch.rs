//! The two clones under `target/upstream/`: `cache` is fetched from upstream and reset freely
//! by `check`; `work` is cloned from `cache` and holds the `unisolver` branch a maintainer edits.
use super::git::{path_str, Git};
use super::{Ctx, REPO_ENV};
use anyhow::{bail, ensure, Context, Result};
use std::path::Path;
use std::time::Duration;

const NET_TIMEOUT: Duration = Duration::from_secs(600);

/// The cache clone, fetched until it has the locked commit.
pub fn ensure_cache(ctx: &Ctx) -> Result<Git> {
    let dir = ctx.cache_dir();
    if !is_repo(&dir) {
        // Missing, or half there (a CI cache of target/ can restore an incomplete .git)
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::create_dir_all(ctx.scratch())?;
        // Clone beside and rename, so an interrupted clone never looks like a cache
        let partial = ctx.scratch().join("cache.partial");
        let _ = std::fs::remove_dir_all(&partial);
        Git::new(ctx.scratch())
            .run_timeout(
                &[
                    "clone",
                    "--quiet",
                    "--no-checkout",
                    &ctx.repo_url,
                    "cache.partial",
                ],
                NET_TIMEOUT,
            )
            .with_context(|| unreachable(ctx))?;
        std::fs::rename(&partial, &dir)?;
    }
    let git = Git::new(&dir);
    git.run(&["remote", "set-url", "origin", &ctx.repo_url])?;
    if !has_commit(&git, &ctx.lock.commit) {
        fetch_tags(ctx, &git)?;
        ensure!(
            has_commit(&git, &ctx.lock.commit),
            "{} has no commit {} (tag {})",
            ctx.repo_url,
            ctx.lock.commit,
            ctx.lock.tag
        );
    }
    Ok(git)
}

/// Fetches every upstream tag into the cache clone.
pub fn fetch_tags(ctx: &Ctx, cache: &Git) -> Result<()> {
    cache
        .run_timeout(
            &["fetch", "--quiet", "--tags", "--force", "origin"],
            NET_TIMEOUT,
        )
        .with_context(|| unreachable(ctx))?;
    Ok(())
}

fn unreachable(ctx: &Ctx) -> String {
    format!(
        "cannot fetch upstream from {} (set {REPO_ENV} to a mirror)",
        ctx.repo_url
    )
}

/// Whether `dir` is a git repository of its own (`Git` keeps git from answering for an
/// enclosing one).
pub fn is_repo(dir: &Path) -> bool {
    dir.is_dir() && Git::new(dir).succeeds(&["rev-parse", "--git-dir"])
}

pub fn has_commit(git: &Git, rev: &str) -> bool {
    git.succeeds(&["cat-file", "-e", &format!("{rev}^{{commit}}")])
}

pub fn has_tag(git: &Git, tag: &str) -> bool {
    git.succeeds(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/tags/{tag}"),
    ])
}

/// The commit a tag points at (annotated tags peeled).
pub fn resolve_tag(git: &Git, tag: &str) -> Result<String> {
    let rev = format!("refs/tags/{tag}^{{commit}}");
    let sha = git.run(&["rev-parse", "--verify", "--quiet", &rev]);
    Ok(sha
        .with_context(|| format!("upstream has no tag {tag}"))?
        .trim()
        .to_string())
}

/// The work clone, holding every tag the cache has.
pub fn ensure_work(ctx: &Ctx) -> Result<Git> {
    ensure_cache(ctx)?;
    let dir = ctx.work_dir();
    if dir.exists() && !is_repo(&dir) {
        bail!(
            "{} is not a git repository: move it away and run `cargo xtask upstream edit` again",
            dir.display()
        );
    }
    if !dir.exists() {
        let cache = ctx.cache_dir();
        Git::new(ctx.scratch()).run(&["clone", "--quiet", path_str(&cache)?, "work"])?;
    }
    let git = Git::new(&dir);
    // origin is the cache: local and offline
    git.run(&["fetch", "--quiet", "--tags", "--force", "origin"])?;
    Ok(git)
}

/// Writes the `include` paths of `rev` into `dest` (an existing directory) through a scratch
/// index, leaving the repository's own index and work tree alone.
pub fn materialize(git: &Git, rev: &str, include: &[String], dest: &Path) -> Result<()> {
    let dest = std::path::absolute(dest)?;
    let index = std::path::absolute(git.dir().join(".git/xtask-materialize.index"))?;
    let _ = std::fs::remove_file(&index);
    let out = git
        .command()
        .env("GIT_INDEX_FILE", &index)
        .arg(format!("--work-tree={}", dest.display()))
        .args(["checkout", rev, "--"])
        .args(include)
        .output()?;
    let _ = std::fs::remove_file(&index);
    if !out.status.success() {
        bail!(
            "git checkout {rev} -- {} failed: {}",
            include.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}
