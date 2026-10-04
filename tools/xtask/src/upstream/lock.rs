//! `third_party/tetra3.lock`: the upstream commit the vendored tree is built from.
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    /// Upstream repository URL.
    pub repo: String,
    /// Release tag, e.g. `v0.13.0`.
    pub tag: String,
    /// The commit `tag` points at (full SHA; annotated tags peeled).
    pub commit: String,
    /// Upstream paths copied into the vendored tree.
    pub include: Vec<String>,
}

impl Lock {
    pub fn parse(text: &str) -> Result<Lock> {
        let lock: Lock = toml::from_str(text).context("parsing the upstream lock")?;
        ensure!(
            lock.commit.len() == 40 && lock.commit.bytes().all(|b| b.is_ascii_hexdigit()),
            "lock: commit must be a full 40-digit SHA, got {:?}",
            lock.commit
        );
        ensure!(!lock.include.is_empty(), "lock: include is empty");
        Ok(lock)
    }

    pub fn read(path: &Path) -> Result<Lock> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Lock::parse(&text).with_context(|| format!("in {}", path.display()))
    }

    /// The file as `write` stores it: fixed field order and comments, so a sync changes only
    /// the `tag` and `commit` lines.
    pub fn render(&self, name: &str) -> String {
        let include: Vec<String> = self.include.iter().map(|p| format!("{p:?}")).collect();
        format!(
            "# Pinned upstream for third_party/{name}, managed by `cargo xtask upstream` (docs/upstream.md)\n\
             repo    = {:?}\n\
             tag     = {:?}\n\
             commit  = {:?}\n\
             # Upstream paths copied into the vendored tree\n\
             include = [{}]\n",
            self.repo,
            self.tag,
            self.commit,
            include.join(", ")
        )
    }

    pub fn write(&self, name: &str, path: &Path) -> Result<()> {
        std::fs::write(path, self.render(name))
            .with_context(|| format!("writing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
repo    = "https://github.com/ssmichael1/tetra3rs"
tag     = "v0.13.0"
commit  = "d43eb49256bdbe1d743be54c1058985ee2a6ae03"
include = ["Cargo.toml", "src"]
"#;

    #[test]
    fn parses_and_renders_round_trip() {
        let lock = Lock::parse(SAMPLE).unwrap();
        assert_eq!(lock.tag, "v0.13.0");
        assert_eq!(lock.include, ["Cargo.toml", "src"]);
        assert_eq!(Lock::parse(&lock.render("tetra3")).unwrap(), lock);
    }

    #[test]
    fn rejects_short_commits_and_unknown_fields() {
        let short = SAMPLE.replace("d43eb49256bdbe1d743be54c1058985ee2a6ae03", "d43eb49");
        assert!(Lock::parse(&short).is_err());
        assert!(Lock::parse(&format!("{SAMPLE}branch = \"main\"\n")).is_err());
    }
}
