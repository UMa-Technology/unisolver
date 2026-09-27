//! `check`, local half: the vendored tree against the locked commit plus the queue.
use super::fetch::{ensure_cache, materialize};
use super::queue::apply_series;
use super::tree::{self, Change};
use super::{read_series, Ctx, VENDORED_KEEP};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct LocalReport {
    /// Problems with the queue itself: `series` and the patch files disagree, or the patches
    /// do not apply.
    pub problems: Vec<String>,
    /// How the vendored tree differs from the locked commit with the queue applied.
    pub changes: Vec<Change>,
}

impl LocalReport {
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty() && self.changes.is_empty()
    }
}

/// Rebuilds the vendored tree in the cache clone (locked commit, `git am` of the series,
/// filtered to `include`) and compares it byte for byte with `third_party/tetra3`, `data/`
/// excluded. An error means the check could not run (upstream unreachable, no cached clone).
pub fn check_local(ctx: &Ctx) -> Result<LocalReport> {
    let mut report = LocalReport::default();
    let series = read_series(ctx)?;
    queue_problems(ctx, &series, &mut report.problems)?;
    if !report.problems.is_empty() {
        return Ok(report);
    }
    let git = ensure_cache(ctx)?;
    git.succeeds(&["am", "--abort"]); // left over from an interrupted check
    git.run(&[
        "checkout",
        "--quiet",
        "--force",
        "--detach",
        &ctx.lock.commit,
    ])?;
    git.run(&["clean", "-fdxq"])?;
    if let Err(e) = apply_series(ctx, &git, &series) {
        report.problems.push(format!("{e:#}"));
        return Ok(report);
    }
    let expected = ctx.expected_dir();
    let _ = std::fs::remove_dir_all(&expected);
    std::fs::create_dir_all(&expected)?;
    materialize(&git, "HEAD", &ctx.lock.include, &expected)?;
    report.changes = tree::compare(&expected, &ctx.vendored(), &[VENDORED_KEEP])?;
    Ok(report)
}

/// `series` entries without a file, and `.patch` files missing from `series`.
fn queue_problems(ctx: &Ctx, series: &[String], problems: &mut Vec<String>) -> Result<()> {
    let dir = ctx.patches_dir();
    for name in series {
        if !dir.join(name).is_file() {
            problems.push(format!("series lists {name}, which does not exist"));
        }
    }
    let mut unlisted = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".patch") && !series.contains(&name) {
            unlisted.push(format!("{name} is not listed in series"));
        }
    }
    unlisted.sort();
    problems.extend(unlisted);
    Ok(())
}
