//! Upstream release tags (`vX.Y.Z`) and `git ls-remote` output.
use std::collections::BTreeMap;

/// `vX.Y.Z` as a comparable triple; anything else (pre-releases, other names) is `None`.
pub fn version(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag.strip_prefix('v')?.split('.');
    let v = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(v)
}

/// Tag name to commit from `git ls-remote --tags` output. Annotated tags resolve to the commit
/// they point at (the `^{}` line), lightweight tags to their own object.
pub fn parse_ls_remote(out: &str) -> BTreeMap<String, String> {
    let mut tags = BTreeMap::new();
    let mut peeled = BTreeMap::new();
    for line in out.lines() {
        let Some((sha, name)) = line.split_once('\t') else {
            continue;
        };
        let Some(name) = name.strip_prefix("refs/tags/") else {
            continue;
        };
        match name.strip_suffix("^{}") {
            Some(base) => peeled.insert(base.to_string(), sha.to_string()),
            None => tags.insert(name.to_string(), sha.to_string()),
        };
    }
    tags.extend(peeled);
    tags
}

/// Release tags newer than `pinned`, oldest first.
pub fn newer_than<'a>(tags: impl IntoIterator<Item = &'a String>, pinned: &str) -> Vec<String> {
    let Some(pinned) = version(pinned) else {
        return Vec::new();
    };
    let mut newer: Vec<_> = tags
        .into_iter()
        .filter_map(|t| version(t).filter(|v| *v > pinned).map(|v| (v, t.clone())))
        .collect();
    newer.sort();
    newer.into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_order_numerically() {
        assert_eq!(version("v0.13.0"), Some((0, 13, 0)));
        assert_eq!(version("v0.13"), None);
        assert_eq!(version("0.13.0"), None);
        assert_eq!(version("v1.0.0-rc1"), None);
        assert!(version("v0.10.0") > version("v0.9.0"));
    }

    #[test]
    fn ls_remote_prefers_peeled_commits() {
        let out = "aaa\trefs/tags/v0.12.0\nbbb\trefs/tags/v0.13.0\nccc\trefs/tags/v0.13.0^{}\n";
        let tags = parse_ls_remote(out);
        assert_eq!(tags["v0.12.0"], "aaa");
        assert_eq!(tags["v0.13.0"], "ccc");
    }

    #[test]
    fn newer_than_skips_older_and_odd_tags() {
        let tags: Vec<String> = [
            "v0.9.0", "v0.14.0", "v0.13.0", "v0.10.0", "v0.13.1", "nightly",
        ]
        .map(String::from)
        .into();
        assert_eq!(
            newer_than(&tags, "v0.10.0"),
            ["v0.13.0", "v0.13.1", "v0.14.0"]
        );
        assert!(newer_than(&tags, "v0.14.0").is_empty());
    }
}
