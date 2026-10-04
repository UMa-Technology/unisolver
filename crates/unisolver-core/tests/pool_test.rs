//! Multi-tier routing with two **independent** synthetic databases: using the wrong tier
//! really fails, so correct routing is observable.
use unisolver_core::*;
use unisolver_synth as synth;

const WIDE: &str = "unisolver_15_40";
const NARROW: &str = "unisolver_8_15";

/// Both tiers are written into a private directory (the one open_dir scans).
/// `OnceLock` plus write-then-rename: tests run in parallel and must never read a
/// half-written database (an `if !exists` check produced "file truncated").
fn tier_dir() -> &'static std::path::Path {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("unisolver_pool_test");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, db) in [(WIDE, synth::test_db()), (NARROW, synth::narrow_test_db())] {
            let dst = dir.join(format!("{name}.db"));
            let tmp = dir.join(format!("{name}.db.tmp{}", std::process::id()));
            db.save_to_file_v2(tmp.to_str().unwrap()).unwrap();
            std::fs::rename(&tmp, &dst).unwrap();
        }
        dir
    })
    .as_path()
}

fn pool() -> SolverPool {
    let (p, skipped) = SolverPool::open_dir(tier_dir().to_str().unwrap()).unwrap();
    assert!(skipped.is_empty(), "skipped: {skipped:?}");
    p
}

fn frame_of(img: Vec<f32>, w: u32, h: u32) -> Frame {
    Frame {
        width: w,
        height: h,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(img),
    }
}

/// A frame rendered from one tier's catalog at the given pointing and FOV
fn render_field(db: &tetra3::SolverDatabase, ra: f64, dec: f64, fov: f32, w: u32, h: u32) -> Frame {
    let q = synth::look_at(ra, dec, 15.0);
    frame_of(
        synth::render(
            db.star_catalog.stars(),
            &q,
            fov,
            w,
            h,
            &synth::RenderParams::default(),
            5,
        ),
        w,
        h,
    )
}

fn base() -> SolveOptions {
    let mut o = SolveOptions::new(20.0);
    o.extraction = ExtractionProfile::CleanSensor; // synthetic = clean sensor
    o.timeout_ms = Some(20_000);
    o
}

#[test]
fn registers_both_tiers_widest_first_and_is_idempotent() {
    let mut p = pool();
    let t = p.tiers();
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].name, WIDE, "widest tier must come first");
    assert_eq!(t[1].name, NARROW);
    assert!((t[0].min_fov_deg - 15.0).abs() < 0.01 && (t[1].max_fov_deg - 15.0).abs() < 0.01);
    // Install-then-register flows easily report the same file twice
    let again = p
        .register(tier_dir().join(format!("{WIDE}.db")).to_str().unwrap())
        .unwrap();
    assert_eq!(again.name, WIDE);
    assert_eq!(p.len(), 2, "re-registering the same file must be a no-op");
}

/// Wide field: the first ladder rung is in the wide tier's range; one hit, the narrow tier untouched.
#[test]
fn wide_field_routes_to_the_wide_tier() {
    let p = pool();
    let f = render_field(synth::test_db(), 120.0, 40.0, 20.0, 1024, 768);
    let r = p
        .solve_auto(&f, &base(), &aspect_ladder(1024, 768))
        .unwrap();
    assert!(
        matches!(r.outcome.status, SolveStatus::Ok),
        "{:?}",
        r.outcome.status
    );
    assert_eq!(r.db.as_deref(), Some(WIDE));
    let g = r.outcome.solution.unwrap();
    assert!((g.ra_deg - 120.0).abs() < 0.1 && (g.dec_deg - 40.0).abs() < 0.1);
    assert!(
        r.attempts.iter().all(|a| a.db == WIDE),
        "narrow tier should never be touched: {:?}",
        r.attempts
    );
}

/// Narrow field without any header hint: the ladder exhausts the wide tier (it must fail, the
/// catalogs differ), then the narrow tier's fill-in finds it. The main "no database named" case.
#[test]
fn narrow_field_falls_through_to_the_narrow_tier() {
    let p = pool();
    let f = render_field(synth::narrow_test_db(), 250.0, -20.0, 10.0, 1024, 768);
    let r = p
        .solve_auto(&f, &base(), &aspect_ladder(1024, 768))
        .unwrap();
    assert!(
        matches!(r.outcome.status, SolveStatus::Ok),
        "{:?}",
        r.outcome.status
    );
    assert_eq!(r.db.as_deref(), Some(NARROW), "attempts: {:?}", r.attempts);
    let g = r.outcome.solution.unwrap();
    assert!((g.ra_deg - 250.0).abs() < 0.1 && (g.dec_deg + 20.0).abs() < 0.1);
    assert!(
        r.attempts.iter().any(|a| a.db == WIDE),
        "the wide tier must be tried first (it owns the ladder head)"
    );
    // A cross-tier ladder extracts once: the reason routing exists (seconds per 26 Mpx frame)
    assert_eq!(r.extract_count, 1, "attempts: {}", r.attempts.len());
    assert!(r.attempts.len() > 1);
}

/// A header hint inside the narrow tier: the first attempt uses it; the wide tier is never tried.
#[test]
fn header_hint_skips_the_wide_sweep() {
    let p = pool();
    let f = render_field(synth::narrow_test_db(), 30.0, 10.0, 10.0, 1024, 768);
    let mut hints = vec![FovPreset {
        fov_deg: 10.0,
        max_error_deg: 1.5,
    }];
    hints.extend(aspect_ladder(1024, 768));
    let r = p.solve_auto(&f, &base(), &hints).unwrap();
    assert!(matches!(r.outcome.status, SolveStatus::Ok));
    assert_eq!(r.attempts.len(), 1, "attempts: {:?}", r.attempts);
    assert_eq!(r.attempts[0].db, NARROW);
}

/// Calibrated camera (FOV from the focal length): no ladder, only tiers covering that FOV.
#[test]
fn calibrated_camera_routes_by_its_own_fov() {
    let p = pool();
    let f = render_field(synth::narrow_test_db(), 200.0, -35.0, 10.0, 1024, 768);
    let mut o = base();
    o.camera = Some(CameraParams::from_horizontal_fov(10.0, 1024, 768).unwrap());
    let r = p.solve_auto(&f, &o, &[]).unwrap();
    assert!(
        matches!(r.outcome.status, SolveStatus::Ok),
        "{:?}",
        r.outcome.status
    );
    assert_eq!(
        r.attempts.len(),
        1,
        "known FOV must not sweep: {:?}",
        r.attempts
    );
    assert_eq!(r.attempts[0].db, NARROW);
}

/// A FOV no tier covers (0.6°): not an error; the nearest tier tries once, then an honest failure.
#[test]
fn fov_outside_every_tier_fails_cleanly() {
    let p = pool();
    let f = render_field(synth::narrow_test_db(), 10.0, 0.0, 0.6, 512, 512);
    let mut o = base();
    o.camera = Some(CameraParams::from_horizontal_fov(0.6, 512, 512).unwrap());
    o.timeout_ms = Some(2000);
    let r = p.solve_auto(&f, &o, &[]).unwrap();
    assert!(!matches!(r.outcome.status, SolveStatus::Ok));
    assert_eq!(r.db, None);
    assert_eq!(r.attempts.len(), 1, "{:?}", r.attempts);
}

/// An empty pool gives a readable error rather than a handle that never solves.
#[test]
fn empty_pool_is_an_error_not_a_silent_no_op() {
    let dir = std::env::temp_dir().join("unisolver_pool_empty");
    std::fs::create_dir_all(&dir).unwrap();
    let e = SolverPool::open_dir(dir.to_str().unwrap()).unwrap_err();
    assert!(e.to_string().contains("no usable database"), "{e}");
    let p = SolverPool::new().unwrap();
    let f = frame_of(vec![0.0; 64 * 64], 64, 64);
    assert!(p.solve_auto(&f, &base(), &[]).is_err());
}

/// A bad file is skipped without sinking the pool (when a good database is in the same directory).
#[test]
fn a_corrupt_file_is_skipped_not_fatal() {
    let dir = std::env::temp_dir().join("unisolver_pool_corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(
        tier_dir().join(format!("{WIDE}.db")),
        dir.join(format!("{WIDE}.db")),
    )
    .unwrap();
    std::fs::write(dir.join("junk.db"), b"not a database at all").unwrap();
    let (p, skipped) = SolverPool::open_dir(dir.to_str().unwrap()).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].0.ends_with("junk.db"));
}

/// Builds without the narrow-field engine refuse a package rather than ignore it
#[cfg(not(feature = "narrow"))]
#[test]
fn builds_without_the_engine_refuse_a_narrow_package() {
    let e = SolverPool::new()
        .unwrap()
        .register_narrow("a.idx", "a.stars")
        .unwrap_err()
        .to_string();
    assert!(e.contains("no narrow-field engine"), "{e}");
}
