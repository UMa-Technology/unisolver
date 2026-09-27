//! Portable monotonic clock for the solve path.
//!
//! On every target with a working `std` clock this is exactly
//! [`std::time::Instant`]. On `wasm32-unknown-unknown` — the browser target —
//! `std::time::Instant::now()` aborts the module ("time not implemented on
//! this platform"), so there we use [`web_time::Instant`], which is backed by
//! `performance.now()` through the JS host. WASI and Emscripten have a real
//! `std` clock and keep it: the substitution is gated on
//! `target_os = "unknown"`, not on `target_arch = "wasm32"` alone.
//!
//! All solver timing — [`SolveConfig::solve_timeout_ms`], the reported
//! `solve_time_ms`, and the `profile`-feature `timed!` spans — goes through
//! this alias so the platform choice is made in one place.
//!
//! A wasm32-unknown-unknown module run *without* a JS host (a bare wasmtime /
//! wasmer embedder) has no `performance.now()` to bind to; on such a host use
//! [`SolveConfig::max_patterns_checked`] to bound the search, since no clock
//! is available to enforce a timeout.
//!
//! [`SolveConfig::solve_timeout_ms`]: super::SolveConfig::solve_timeout_ms
//! [`SolveConfig::max_patterns_checked`]: super::SolveConfig::max_patterns_checked

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use std::time::Instant;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use web_time::Instant;
