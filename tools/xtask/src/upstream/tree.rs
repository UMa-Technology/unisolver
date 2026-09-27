//! Byte-for-byte comparison of two directory trees.
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// In both trees, with different bytes.
    Modified(String),
    /// Only in the actual tree.
    Extra(String),
    /// Only in the expected tree.
    Missing(String),
}

impl Change {
    pub fn path(&self) -> &str {
        match self {
            Change::Modified(p) | Change::Extra(p) | Change::Missing(p) => p,
        }
    }
}

/// Files under `root` as `/`-separated relative paths, skipping the top-level entries named in
/// `skip_top` and Finder's `.DS_Store` files.
pub fn files(root: &Path, skip_top: &[&str]) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    walk(root, root, skip_top, &mut out)?;
    Ok(out)
}

fn walk(root: &Path, dir: &Path, skip_top: &[&str], out: &mut BTreeSet<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".DS_Store" || (dir == root && skip_top.contains(&name.as_str())) {
            continue;
        }
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk(root, &path, skip_top, out)?;
        } else {
            let rel = path.strip_prefix(root).expect("walk stays under root");
            let parts: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            out.insert(parts.join("/"));
        }
    }
    Ok(())
}

/// How `actual` differs from `expected`, sorted by path. `skip_top` names top-level entries of
/// `actual` that take no part in the comparison.
pub fn compare(expected: &Path, actual: &Path, skip_top: &[&str]) -> Result<Vec<Change>> {
    let want = files(expected, &[])?;
    let have = files(actual, skip_top)?;
    let mut changes = Vec::new();
    for p in want.union(&have) {
        let change = match (want.contains(p), have.contains(p)) {
            (true, false) => Change::Missing(p.clone()),
            (false, true) => Change::Extra(p.clone()),
            _ => {
                let a = std::fs::read(expected.join(p)).with_context(|| format!("reading {p}"))?;
                let b = std::fs::read(actual.join(p)).with_context(|| format!("reading {p}"))?;
                if a == b {
                    continue;
                }
                Change::Modified(p.clone())
            }
        };
        changes.push(change);
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn reports_modified_extra_and_missing_files() {
        let (e, a) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(e.path(), "src/lib.rs", "fn a() {}\n");
        write(a.path(), "src/lib.rs", "fn b() {}\n");
        write(e.path(), "README.md", "same");
        write(a.path(), "README.md", "same");
        write(e.path(), "LICENSE", "only expected");
        write(a.path(), "src/new.rs", "only actual");
        assert_eq!(
            compare(e.path(), a.path(), &[]).unwrap(),
            [
                Change::Missing("LICENSE".into()),
                Change::Modified("src/lib.rs".into()),
                Change::Extra("src/new.rs".into()),
            ]
        );
    }

    #[test]
    fn skips_named_top_level_entries_and_ds_store() {
        let (e, a) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write(e.path(), "src/lib.rs", "x");
        write(a.path(), "src/lib.rs", "x");
        write(a.path(), "data/cache.bin", "test data");
        write(a.path(), ".DS_Store", "finder");
        write(a.path(), "src/.DS_Store", "finder");
        assert!(compare(e.path(), a.path(), &["data"]).unwrap().is_empty());
    }
}
