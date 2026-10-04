//! Star-pattern matching for unisolver's narrow-field engine: a star-centred blind pattern
//! index ([`blind`]), hinted triangle matching ([`solve`]), packed star tiles ([`catalog`])
//! and the TAN/SIP [`Wcs`].
//!
//! Derived from seiza v0.19.2 (<https://github.com/theatrus/seiza>), Copyright Yann Ramin,
//! licensed under the Apache License 2.0 (see `LICENSE`). Changes for unisolver: only the
//! modules above are kept (no star detection, object catalogs or data paths), and solves take
//! optional deadlines (`solve_until`, `solve_blind_until`, [`Error::Timeout`]) that the blind
//! search also checks while it enumerates patterns. File formats: blind indexes
//! ([`blind::INDEX_MAGIC`], `UNIBLIX1`) and star tiles ([`catalog::TILE_MAGIC`], `UNISTAR1`),
//! seiza's layouts under unisolver's own headers.

pub mod blind;
pub mod catalog;
pub mod detect;
pub mod solve;
pub mod wcs;

pub use detect::DetectedStar;
pub use wcs::{FitsCardValue, Sip, Wcs};

/// What [`blind::BlindIndex::build`] writes: the same inputs give the same bytes for the same
/// value (here seiza 0.19.2's builder output with unisolver's index header). Package reports
/// record it; change it, and the hash in `tests/golden.rs`, whenever a change alters the bytes.
pub const INDEX_BUILDER: &str = "starmatch-1";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("catalog error: {0}")]
    Catalog(String),
    #[error("solve failed: {0}")]
    Solve(String),
    #[error("solve timed out")]
    Timeout,
}

/// Whether an optional solve deadline has passed.
pub(crate) fn deadline_passed(deadline: Option<std::time::Instant>) -> bool {
    deadline.is_some_and(|d| std::time::Instant::now() >= d)
}
