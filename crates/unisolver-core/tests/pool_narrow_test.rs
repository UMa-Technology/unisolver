//! The pool with the narrow-field engine: registration by file header, and the routing rules
//! that keep the tetra3 tiers' fast path as it was (`narrow::route`).
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use unisolver_core::narrow::testkit::{patch_sky, write_package};
use unisolver_core::*;
use unisolver_starmatch::catalog::{MemoryCatalog, StarCatalog};
use unisolver_synth as synth;

const WIDE: &str = "unisolver_15_40";
const MID: &str = "unisolver_8_15";
const PACKAGE: &str = "patch";
/// Centre of the narrow package's sky: a dense patch of other stars than the tetra3 tiers', so
/// each engine fails on the other's frames
const PATCH: (f64, f64) = (80.0, 30.0);

fn patch() -> &'static MemoryCatalog {
    static SKY: OnceLock<MemoryCatalog> = OnceLock::new();
    SKY.get_or_init(|| patch_sky(PATCH, 5.0, 60.0, 21))
}

/// Both synthetic tetra3 tiers and the narrow package, in the directory `open_dir` scans
fn data_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("unisolver_pool_narrow_test");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, db) in [(WIDE, synth::test_db()), (MID, synth::narrow_test_db())] {
            let tmp = dir.join(format!("{name}.db.tmp{}", std::process::id()));
            db.save_to_file(tmp.to_str().unwrap()).unwrap();
            std::fs::rename(&tmp, dir.join(format!("{name}.db"))).unwrap();
        }
        write_package(patch(), (1.0, 15.0), &dir, PACKAGE);
        dir
    })
}

fn package() -> (String, String) {
    let at = |ext: &str| {
        data_dir()
            .join(format!("{PACKAGE}.{ext}"))
            .to_string_lossy()
            .to_string()
    };
    (at("idx"), at("stars"))
}

fn full_pool() -> SolverPool {
    let (p, skipped) = SolverPool::open_dir(data_dir().to_str().unwrap()).unwrap();
    assert!(skipped.is_empty(), "{skipped:?}");
    p
}

fn tetra3_pool() -> SolverPool {
    let mut p = SolverPool::new().unwrap();
    for name in [WIDE, MID] {
        p.register(data_dir().join(format!("{name}.db")).to_str().unwrap())
            .unwrap();
    }
    p
}

/// A fresh directory under the temp dir holding copies of `files`
fn dir_with(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{name}_{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    for (from, to) in files {
        std::fs::copy(data_dir().join(from), dir.join(to)).unwrap();
    }
    dir
}

#[test]
fn open_dir_recognizes_the_package_by_its_headers() {
    let p = full_pool();
    let t = p.tiers();
    let got: Vec<(&str, TierKind)> = t.iter().map(|t| (t.name.as_str(), t.kind)).collect();
    assert_eq!(
        got,
        [
            (WIDE, TierKind::Tetra3),
            (MID, TierKind::Tetra3),
            (PACKAGE, TierKind::Narrow)
        ]
    );
    assert!(
        (t[2].min_fov_deg - 0.6).abs() < 1e-6,
        "{}",
        t[2].min_fov_deg
    );
    assert!((t[2].max_fov_deg - narrow::NARROW_MAX_FOV_DEG).abs() < 1e-6);
    assert_eq!(p.len(), 3);
    // File names play no part: renamed files still pair, as the only index and star file
    let dir = dir_with(
        "unisolver_pool_narrow_renamed",
        &[("patch.idx", "a.bin"), ("patch.stars", "b.dat")],
    );
    let (p, skipped) = SolverPool::open_dir(dir.to_str().unwrap()).unwrap();
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(p.tiers()[0].kind, TierKind::Narrow);
}

#[test]
fn unpaired_narrow_files_are_skipped() {
    let dir = dir_with(
        "unisolver_pool_narrow_unpaired",
        &[
            (&format!("{WIDE}.db"), &format!("{WIDE}.db")),
            ("patch.idx", "one.idx"),
            ("patch.stars", "one.stars"),
            ("patch.stars", "two.stars"),
        ],
    );
    let (p, skipped) = SolverPool::open_dir(dir.to_str().unwrap()).unwrap();
    assert_eq!(p.len(), 2, "{:?}", p.tiers());
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].0.ends_with("two.stars"), "{skipped:?}");
    assert!(skipped[0].1.contains("without a matching"), "{skipped:?}");
}

#[test]
fn registering_again_is_idempotent_and_a_second_package_is_refused() {
    let mut p = tetra3_pool();
    let (idx, stars) = package();
    let a = p.register_narrow(&idx, &stars).unwrap();
    let b = p.register_narrow(&idx, &stars).unwrap();
    assert_eq!(a.path, b.path);
    assert_eq!(a.kind, TierKind::Narrow);
    assert_eq!(p.len(), 3);

    let other = std::env::temp_dir().join(format!(
        "unisolver_pool_narrow_other_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&other).unwrap();
    let small = patch_sky((200.0, -40.0), 2.0, 20.0, 9);
    let (i2, s2) = write_package(&small, (1.0, 15.0), &other, "other");
    let e = p
        .register_narrow(i2.to_str().unwrap(), s2.to_str().unwrap())
        .unwrap_err()
        .to_string();
    assert!(e.contains("already registered"), "{e}");
    assert_eq!(p.len(), 3);
}

/// Registration maps the files and reads their headers only: a 4 GB index (sparse, so it costs
/// no disk) registers as fast as a small one
#[cfg(unix)]
#[test]
fn registration_reads_only_the_headers() {
    use std::io::Write;
    let dir = std::env::temp_dir().join(format!(
        "unisolver_pool_narrow_sparse_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let idx = dir.join("big.idx");
    let patterns: u64 = 97_000_000; // 44 bytes each
    let mut header = [0u8; 64];
    header[..8].copy_from_slice(b"SEIZABI1");
    header[24..32].copy_from_slice(&patterns.to_le_bytes());
    header[32..36].copy_from_slice(&16.0f32.to_le_bytes());
    header[36..40].copy_from_slice(&6.0f32.to_le_bytes());
    header[40..44].copy_from_slice(&128u32.to_le_bytes());
    header[44..48].copy_from_slice(&1u32.to_le_bytes());
    let mut f = std::fs::File::create(&idx).unwrap();
    f.write_all(&header).unwrap();
    // No keys: one 4-byte candidate offset, then the patterns
    f.set_len(64 + 4 + patterns * 44).unwrap();
    drop(f);

    let stars = package().1; // builds the test data outside the timing
    let mut p = SolverPool::new().unwrap();
    let t0 = std::time::Instant::now();
    let info = p.register_narrow(idx.to_str().unwrap(), &stars).unwrap();
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    std::fs::remove_dir_all(&dir).ok();
    assert!(ms < 100.0, "registering a 4 GB index took {ms:.0} ms");
    assert_eq!(info.num_patterns as u64, patterns);
    assert!(
        (info.min_fov_deg - 0.18).abs() < 1e-6,
        "{}",
        info.min_fov_deg
    );
}

#[test]
fn narrow_solves_annotate_with_the_narrowest_tetra3_tier() {
    let p = full_pool();
    let min_fov = |db: Option<&str>| p.annotation_solver(db).map(|s| s.properties().min_fov_deg);
    let near = |v: Option<f32>, want: f32| v.is_some_and(|v| (v - want).abs() < 0.01);
    assert!(near(min_fov(None), 15.0), "widest by default");
    assert!(near(min_fov(Some(WIDE)), 15.0));
    assert!(near(min_fov(Some(MID)), 8.0));
    assert!(near(min_fov(Some(PACKAGE)), 8.0), "narrowest tetra3 tier");
    assert!(min_fov(Some("nope")).is_none());
}

const W: u32 = 1600;
const H: u32 = 1200;

fn frame_of(img: Vec<f32>, w: u32, h: u32) -> Frame {
    Frame {
        width: w,
        height: h,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(img),
    }
}

fn as_tetra3(sky: &MemoryCatalog) -> Vec<tetra3::Star> {
    sky.all_brighter_than(30.0)
        .iter()
        .enumerate()
        .map(|(i, s)| tetra3::Star {
            id: i as i64 + 1,
            ra_rad: s.ra.to_radians() as f32,
            dec_rad: s.dec.to_radians() as f32,
            mag: s.mag,
        })
        .collect()
}

/// A W×H frame of `sky` down to its faintest stars, centred on (ra, dec), `fov` degrees wide
fn narrow_frame(sky: &MemoryCatalog, ra: f64, dec: f64, fov: f32) -> Frame {
    let params = synth::RenderParams {
        mag_limit: 12.5,
        flux_mag8: 20_000.0,
        ..Default::default()
    };
    let q = synth::look_at(ra, dec, 30.0);
    frame_of(
        synth::render(&as_tetra3(sky), &q, fov, W, H, &params, 3),
        W,
        H,
    )
}

/// Where the 2° frames of the package's sky point
const AIM: (f64, f64) = (PATCH.0 + 0.3, PATCH.1 - 0.2);

fn patch_frame() -> Frame {
    narrow_frame(patch(), AIM.0, AIM.1, 2.0)
}

/// A frame of a tetra3 tier's catalog, as `pool_test` renders them
fn tetra3_frame(db: &tetra3::SolverDatabase, ra: f64, dec: f64, fov: f32) -> Frame {
    let q = synth::look_at(ra, dec, 15.0);
    let img = synth::render(
        db.star_catalog.stars(),
        &q,
        fov,
        1024,
        768,
        &synth::RenderParams::default(),
        5,
    );
    frame_of(img, 1024, 768)
}

fn dark_frame() -> Frame {
    let q = synth::look_at(0.0, 0.0, 0.0);
    frame_of(
        synth::render(&[], &q, 2.0, W, H, &synth::RenderParams::default(), 9),
        W,
        H,
    )
}

fn base() -> SolveOptions {
    let mut o = SolveOptions::new(20.0);
    o.extraction = ExtractionProfile::CleanSensor;
    o.timeout_ms = Some(20_000);
    o
}

fn kinds(r: &PoolOutcome) -> Vec<TierKind> {
    r.attempts.iter().map(|a| a.kind).collect()
}

fn route_of(r: &PoolOutcome) -> Vec<(String, f32, SolveStatus)> {
    r.attempts
        .iter()
        .map(|a| (a.db.clone(), a.fov_deg, a.status))
        .collect()
}

fn off_arcmin(ra: f64, dec: f64, at: (f64, f64)) -> f64 {
    let (r1, d1, r2, d2) = (
        ra.to_radians(),
        dec.to_radians(),
        at.0.to_radians(),
        at.1.to_radians(),
    );
    let h = ((d2 - d1) / 2.0).sin().powi(2) + d1.cos() * d2.cos() * ((r2 - r1) / 2.0).sin().powi(2);
    (2.0 * h.sqrt().asin()).to_degrees() * 60.0
}

/// The fast-path invariant: with the FOV unknown, a pool with the engine tries exactly what the
/// pool without it tries, solved or not
#[test]
fn unknown_fovs_route_exactly_as_without_the_engine() {
    let (with, without) = (full_pool(), tetra3_pool());
    let frames = [
        tetra3_frame(synth::test_db(), 120.0, 40.0, 20.0),
        tetra3_frame(synth::narrow_test_db(), 250.0, -20.0, 10.0),
        patch_frame(),
        dark_frame(),
    ];
    for f in &frames {
        let ladder = aspect_ladder(f.width, f.height);
        let a = with.solve_auto(f, &base(), &ladder).unwrap();
        let b = without.solve_auto(f, &base(), &ladder).unwrap();
        assert_eq!(route_of(&a), route_of(&b));
        assert_eq!(a.db, b.db);
        assert!(!kinds(&a).contains(&TierKind::Narrow));
    }
}

#[test]
fn narrow_blind_runs_once_after_every_tetra3_attempt() {
    let f = patch_frame();
    let ladder = aspect_ladder(W, H);
    let plain = tetra3_pool().solve_auto(&f, &base(), &ladder).unwrap();
    assert_ne!(plain.outcome.status, SolveStatus::Ok);
    let mut o = base();
    o.narrow_blind = true;
    let r = full_pool().solve_auto(&f, &o, &ladder).unwrap();
    assert_eq!(r.outcome.status, SolveStatus::Ok, "{:?}", r.attempts);
    assert_eq!(r.db.as_deref(), Some(PACKAGE));
    let n = plain.attempts.len();
    assert_eq!(
        route_of(&r)[..n],
        route_of(&plain)[..],
        "tetra3 first, as before"
    );
    assert_eq!(kinds(&r)[n..], [TierKind::Narrow]);
    let g = r.outcome.solution.unwrap();
    assert!(off_arcmin(g.ra_deg, g.dec_deg, AIM) < 1.0);
    assert!((g.fov_deg - 2.0).abs() < 0.02, "{}", g.fov_deg);
    assert_eq!(
        r.extract_count, plain.extract_count,
        "one extraction serves both"
    );
}

#[test]
fn a_known_fov_no_tetra3_tier_covers_goes_to_the_engine_first() {
    let hints = [FovPreset {
        fov_deg: 2.0,
        max_error_deg: 0.3,
    }];
    let r = full_pool()
        .solve_auto(&patch_frame(), &base(), &hints)
        .unwrap();
    assert_eq!(r.outcome.status, SolveStatus::Ok, "{:?}", r.attempts);
    assert_eq!(kinds(&r), [TierKind::Narrow]);
    assert_eq!(r.attempts[0].fov_deg, 2.0);
    assert_eq!(r.extract_count, 1);

    // With a pointing hint the engine searches around it first; far off, it still solves blind
    for (ra, dec) in [(AIM.0 + 0.8, AIM.1), (AIM.0 - 40.0, AIM.1 + 20.0)] {
        let mut o = base();
        o.pointing_hint = Some(PointingHint {
            ra_deg: ra,
            dec_deg: dec,
            radius_deg: None,
        });
        let r = full_pool().solve_auto(&patch_frame(), &o, &hints).unwrap();
        assert_eq!(
            r.outcome.status,
            SolveStatus::Ok,
            "hint {ra} {dec}: {:?}",
            r.attempts
        );
        let g = r.outcome.solution.unwrap();
        assert!(off_arcmin(g.ra_deg, g.dec_deg, AIM) < 1.0);
    }
}

#[test]
fn tracking_and_wide_fovs_never_reach_the_engine() {
    let p = full_pool();
    // Tracking: an attitude hint and a narrow FOV no tetra3 tier covers
    let mut o = base();
    o.attitude_hint = Some(test_support::wxyz(&synth::look_at(AIM.0, AIM.1, 30.0)));
    o.fov_estimate_deg = 2.0;
    o.fov_max_error_deg = Some(0.3);
    o.narrow_blind = true;
    let r = p.solve_auto(&patch_frame(), &o, &[]).unwrap();
    assert!(!kinds(&r).contains(&TierKind::Narrow), "{:?}", r.attempts);

    // A known 10° frame: a tetra3 tier solves it and the engine is never asked
    let mut o = base();
    o.narrow_blind = true;
    let f = tetra3_frame(synth::narrow_test_db(), 250.0, -20.0, 10.0);
    let hint = [FovPreset {
        fov_deg: 10.0,
        max_error_deg: 1.5,
    }];
    let r = p.solve_auto(&f, &o, &hint).unwrap();
    assert_eq!(r.db.as_deref(), Some(MID));
    assert!(!kinds(&r).contains(&TierKind::Narrow));

    // A known 5° frame nothing solves: above the engine's range, so it fails as before
    let hint = [FovPreset {
        fov_deg: 5.0,
        max_error_deg: 0.75,
    }];
    let r = p.solve_auto(&patch_frame(), &o, &hint).unwrap();
    assert_ne!(r.outcome.status, SolveStatus::Ok);
    assert!(!kinds(&r).contains(&TierKind::Narrow));
}

#[test]
fn frames_with_too_few_centroids_skip_the_engine() {
    let p = full_pool();
    let mut o = base();
    o.narrow_blind = true;
    for hints in [
        aspect_ladder(W, H),
        vec![FovPreset {
            fov_deg: 2.0,
            max_error_deg: 0.3,
        }],
    ] {
        let r = p.solve_auto(&dark_frame(), &o, &hints).unwrap();
        assert!(!kinds(&r).contains(&TierKind::Narrow), "{:?}", r.attempts);
    }
}

#[test]
fn the_fallback_keeps_to_its_budget() {
    // Stars the package does not hold: the engine fails, within its budget
    let elsewhere = patch_sky((250.0, 60.0), 3.0, 60.0, 77);
    let f = narrow_frame(&elsewhere, 250.0, 60.0, 2.0);
    let mut o = base();
    o.narrow_blind = true;
    o.narrow_fallback_ms = 300;
    let r = full_pool()
        .solve_auto(&f, &o, &aspect_ladder(W, H))
        .unwrap();
    let last = r.attempts.last().unwrap();
    assert_eq!(last.kind, TierKind::Narrow, "{:?}", r.attempts);
    assert!(matches!(
        last.status,
        SolveStatus::NoMatch | SolveStatus::Timeout
    ));
    assert!(last.solve_ms < 500.0, "{} ms", last.solve_ms);
    // 0 never runs it after the tetra3 tiers
    o.narrow_fallback_ms = 0;
    let r = full_pool()
        .solve_auto(&f, &o, &aspect_ladder(W, H))
        .unwrap();
    assert!(!kinds(&r).contains(&TierKind::Narrow));
}

#[test]
fn a_bad_pointing_hint_is_an_error() {
    let mut o = base();
    o.pointing_hint = Some(PointingHint {
        ra_deg: 10.0,
        dec_deg: 100.0,
        radius_deg: None,
    });
    let Err(e) = full_pool().solve_auto(&patch_frame(), &o, &aspect_ladder(W, H)) else {
        panic!("a pointing hint off the sphere must be refused");
    };
    assert!(e.to_string().contains("pointing_hint"), "{e}");
}
