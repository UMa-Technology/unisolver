//! The vendored tetra3rs copy as a patch queue: `third_party/tetra3` is always the locked
//! upstream commit, filtered to the lock's `include` paths, with the patches in
//! `third_party/tetra3-patches/series` applied. See docs/upstream.md.
pub mod changelog;
pub mod lock;
pub mod tags;
pub mod tree;
