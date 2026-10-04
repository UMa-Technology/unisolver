//! `edit` turns the queue into commits on the `unisolver` branch of the work clone; `export`
//! turns that branch back into the vendored tree and the patch files.
use super::fetch::{ensure_work, is_repo, materialize};
use super::git::{path_str, Git, IDENTITY_EMAIL, IDENTITY_NAME};
use super::{read_series, Ctx};
use anyhow::{bail, ensure, Result};
use std::path::{Path, PathBuf};

/// The branch in the work clone that carries one commit per patch.
pub const BRANCH: &str = "unisolver";

/// Rebuilds `unisolver` in the work clone as the locked commit plus one commit per patch and
/// checks it out; returns the work clone's path. Refuses to drop commits that were never
/// exported (the branch's tree differs from the rebuilt one) or uncommitted changes, unless
/// `force`.
pub fn edit(ctx: &Ctx, force: bool) -> Result<PathBuf> {
    let git = ensure_work(ctx)?;
    ensure_idle(&git, force)?;
    let series = read_series(ctx)?;
    git.run(&[
        "checkout",
        "--quiet",
        "--force",
        "--detach",
        &ctx.lock.commit,
    ])?;
    git.run(&["clean", "-fdq"])?;
    apply_series(ctx, &git, &series)?;
    let rebuilt = git.run(&["rev-parse", "HEAD^{tree}"])?;
    let branch_ref = format!("refs/heads/{BRANCH}");
    if !force && git.succeeds(&["rev-parse", "--verify", "--quiet", &branch_ref]) {
        let old = git.run(&["rev-parse", &format!("{BRANCH}^{{tree}}")])?;
        if old != rebuilt {
            git.run(&["checkout", "--quiet", BRANCH])?;
            bail!(
                "branch {BRANCH} in {} has changes that were never exported: run \
                 `cargo xtask upstream export`, or `edit --force` to discard them",
                git.dir().display()
            );
        }
    }
    git.run(&["checkout", "--quiet", "-B", BRANCH])?;
    Ok(ctx.work_dir())
}

/// Writes `unisolver` back: its tree, filtered to `include`, replaces the vendored tree (except
/// `data/`), and its commits since the locked commit become the patch files and `series`.
/// Returns the number of patches.
pub fn export(ctx: &Ctx) -> Result<usize> {
    let work = ctx.work_dir();
    ensure!(
        is_repo(&work),
        "no work clone at {}: run `cargo xtask upstream edit` first",
        work.display()
    );
    let git = Git::new(&work);
    ensure_idle(&git, false)?;
    let base = ctx.lock.commit.as_str();
    ensure!(
        git.succeeds(&["merge-base", "--is-ancestor", base, BRANCH]),
        "branch {BRANCH} is not based on {} ({base})",
        ctx.lock.tag
    );
    // Paths outside `include` are not vendored: a patch touching them would be lost
    let changed = git.run(&["diff", "--name-only", "--no-renames", base, BRANCH])?;
    let outside: Vec<&str> = changed
        .lines()
        .filter(|p| !included(p, &ctx.lock.include))
        .collect();
    ensure!(
        outside.is_empty(),
        "the patches change paths that are not vendored: {}",
        outside.join(", ")
    );

    let vendored = ctx.vendored();
    std::fs::create_dir_all(&vendored)?;
    for entry in std::fs::read_dir(&vendored)? {
        let entry = entry?;
        if ctx.spec.keep.iter().any(|k| entry.file_name() == *k) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    materialize(&git, BRANCH, &ctx.lock.include, &vendored)?;

    let dir = ctx.patches_dir();
    std::fs::create_dir_all(&dir)?;
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "patch") {
            std::fs::remove_file(path)?;
        }
    }
    let range = format!("{base}..{BRANCH}");
    // Every option that user configuration could change is pinned, so any maintainer's export
    // is byte-identical
    let written = git.run(&[
        "-c",
        "diff.noprefix=false",
        "-c",
        "diff.mnemonicPrefix=false",
        "-c",
        "diff.algorithm=myers",
        "-c",
        "diff.renames=true",
        "format-patch",
        "--zero-commit",
        "--no-signature",
        "--no-stat",
        "-N",
        "--full-index",
        "--subject-prefix=PATCH",
        "--suffix=.patch",
        "--no-cover-letter",
        "--no-thread",
        "--no-to",
        "--no-cc",
        "--no-add-header",
        "--no-base",
        "--no-signoff",
        "--no-notes",
        "-o",
        path_str(&dir)?,
        &range,
    ])?;
    let mut names = Vec::new();
    for line in written.lines().filter(|l| !l.is_empty()) {
        let path = Path::new(line);
        normalize_author(path)?;
        names.push(
            path.file_name()
                .expect("patch file name")
                .to_string_lossy()
                .into_owned(),
        );
    }
    let series: String = names.iter().map(|n| format!("{n}\n")).collect();
    std::fs::write(dir.join("series"), series)?;
    Ok(names.len())
}

/// Applies the series on top of the current HEAD with `git am`; aborts the am on failure.
pub(crate) fn apply_series(ctx: &Ctx, git: &Git, series: &[String]) -> Result<()> {
    if series.is_empty() {
        return Ok(());
    }
    let paths: Vec<PathBuf> = series.iter().map(|n| ctx.patches_dir().join(n)).collect();
    let mut args = vec!["am", "--quiet", "--no-3way"];
    for p in &paths {
        args.push(path_str(p)?);
    }
    if let Err(e) = git.run(&args) {
        git.succeeds(&["am", "--abort"]);
        return Err(e.context(format!("the queue does not apply to {}", ctx.lock.tag)));
    }
    Ok(())
}

/// Refuses while a rebase or am is in progress, or while there are uncommitted changes; with
/// `force`, aborts the former and lets the caller discard the latter.
pub(crate) fn ensure_idle(git: &Git, force: bool) -> Result<()> {
    if rebase_in_progress(git)? {
        if !force {
            bail!(
                "a rebase or am is in progress in {}: finish or abort it first",
                git.dir().display()
            );
        }
        git.succeeds(&["rebase", "--abort"]);
        git.succeeds(&["am", "--abort"]);
    }
    if !force && !git.run(&["status", "--porcelain"])?.trim().is_empty() {
        bail!(
            "{} has uncommitted changes: commit them, or pass --force to discard them",
            git.dir().display()
        );
    }
    Ok(())
}

pub(crate) fn rebase_in_progress(git: &Git) -> Result<bool> {
    for state in ["rebase-merge", "rebase-apply"] {
        let path = git.run(&["rev-parse", "--git-path", state])?;
        if git.dir().join(path.trim()).exists() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn included(path: &str, include: &[String]) -> bool {
    include.iter().any(|i| {
        path == i
            || path
                .strip_prefix(i.as_str())
                .is_some_and(|r| r.starts_with('/'))
    })
}

/// Replaces the `From:` header (and any folded continuation lines) with the fixed identity.
fn normalize_author(path: &Path) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    let mut out = String::with_capacity(text.len());
    let (mut replaced, mut folding) = (false, false);
    for line in text.split_inclusive('\n') {
        if folding && (line.starts_with(' ') || line.starts_with('\t')) {
            continue;
        }
        folding = false;
        if !replaced && line.starts_with("From: ") {
            out.push_str(&format!("From: {IDENTITY_NAME} <{IDENTITY_EMAIL}>\n"));
            replaced = true;
            folding = true;
        } else {
            out.push_str(line);
        }
    }
    std::fs::write(path, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn included_matches_whole_path_components() {
        let include = ["Cargo.toml".to_string(), "src".to_string()];
        assert!(included("Cargo.toml", &include));
        assert!(included("src/solver/mod.rs", &include));
        assert!(!included("srcx/lib.rs", &include));
        assert!(!included("python/demo.py", &include));
    }

    #[test]
    fn normalize_author_replaces_folded_from_header() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("0001-x.patch");
        std::fs::write(&p, "From 0000 Mon Sep 17 00:00:00 2001\nFrom: A Very\n Long Name <a@b.c>\nDate: x\nSubject: [PATCH] x\n\nFrom: body line stays\n").unwrap();
        normalize_author(&p).unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "From 0000 Mon Sep 17 00:00:00 2001\nFrom: unisolver <patches@unisolver.invalid>\nDate: x\nSubject: [PATCH] x\n\nFrom: body line stays\n"
        );
    }
}
