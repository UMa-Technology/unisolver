//! EXIF end to end on synthetic fields: a photo's 35 mm focal length puts the right rung
//! first (file entry, and the frame entry given the focal length by the caller), and its
//! capture time reaches the solve and the outcome.
use image::codecs::jpeg::JpegEncoder;
use image::ImageEncoder;
use unisolver_core::*;
use unisolver_synth as synth;
use unisolver_synth::exif::{jpeg_with_exif, ExifFields};

const W: u32 = 1024;
const H: u32 = 768;
/// 65 mm equivalent on 4:3 is a 29.8° horizontal field: inside the 15–40° test tier and
/// away from the landscape ladder's rungs (70/55 are out of range; 42 is the first tried)
const FOCAL_35MM: u16 = 65;

fn fov_deg() -> f32 {
    focal_35mm_hint(FOCAL_35MM as f32, W, H).unwrap().fov_deg
}

fn field() -> Vec<f32> {
    let q = synth::look_at(200.0, 25.0, 30.0);
    synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        fov_deg(),
        W,
        H,
        &synth::RenderParams::default(),
        11,
    )
}

fn jpeg(img: &[f32]) -> Vec<u8> {
    let max = img.iter().copied().fold(f32::MIN, f32::max);
    let bytes: Vec<u8> = img
        .iter()
        .map(|&v| (v / max * 255.0).clamp(0.0, 255.0) as u8)
        .collect();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 95)
        .write_image(&bytes, W, H, image::ExtendedColorType::L8)
        .unwrap();
    out
}

fn exif() -> ExifFields {
    ExifFields {
        make: Some("Synth".into()),
        model: Some("Phone".into()),
        focal_35mm: Some(FOCAL_35MM),
        date_time_original: Some("2026:03:19 08:03:09".into()),
        offset_time_original: Some("+08:00".into()),
        gps_position: Some((
            "N".into(),
            [(31, 1), (12, 1), (0, 1)],
            "E".into(),
            [(121, 1), (30, 1), (0, 1)],
            false,
            (10, 1),
        )),
        ..Default::default()
    }
}

/// 2026-03-19T00:03:09Z
const TAKEN_UTC_MS: i64 = 1_773_878_589_000;

fn write(name: &str, bytes: &[u8]) -> String {
    let path =
        std::env::temp_dir().join(format!("unisolver_exif_test_{}_{name}", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

fn pool() -> SolverPool {
    let mut p = SolverPool::new().unwrap();
    p.register(&synth::test_db_file("unisolver_15_40_exif.db"))
        .unwrap();
    p
}

fn base() -> SolveOptions {
    let mut o = SolveOptions::new(20.0);
    o.extraction = ExtractionProfile::CleanSensor;
    o.timeout_ms = Some(20_000);
    o
}

fn assert_solved_first(r: &PoolOutcome) {
    assert!(
        matches!(r.outcome.status, SolveStatus::Ok),
        "{:?}",
        r.attempts
    );
    assert_eq!(
        r.attempts.len(),
        1,
        "the hint rung solves it: {:?}",
        r.attempts
    );
    assert!((r.attempts[0].fov_deg - fov_deg()).abs() < 0.01);
    let g = r.outcome.solution.as_ref().unwrap();
    assert!((g.ra_deg - 200.0).abs() < 0.1 && (g.dec_deg - 25.0).abs() < 0.1);
}

#[test]
fn file_entry_takes_the_exif_rung_first_and_reports_the_time() {
    let img = field();
    let p = pool();
    let with = write("with.jpg", &jpeg_with_exif(&jpeg(&img), &exif()));
    let r = p.solve_image_file_auto(&with, &base()).unwrap();
    assert_solved_first(&r);
    assert_eq!(r.outcome.observation_unix_ms, Some(TAKEN_UTC_MS));
    let o = r
        .outcome
        .observer
        .expect("the GPS position comes back for the annotator");
    assert!((o.lat_deg - 31.2).abs() < 1e-9 && (o.lon_deg - 121.5).abs() < 1e-9);

    // The same pixels without EXIF start from the ladder head in range (42°)
    let without = write("without.jpg", &jpeg(&img));
    let r = p.solve_image_file_auto(&without, &base()).unwrap();
    assert!(
        matches!(r.outcome.status, SolveStatus::Ok),
        "{:?}",
        r.attempts
    );
    assert_eq!(r.attempts[0].fov_deg, 42.0, "{:?}", r.attempts);
    assert_eq!(r.outcome.observation_unix_ms, None);
    assert_eq!(r.outcome.observer, None);

    // A time from the caller wins over the header's
    let mut o = base();
    o.observation_unix_ms = Some(42);
    let r = p.solve_image_file_auto(&with, &o).unwrap();
    assert_eq!(r.outcome.observation_unix_ms, Some(42));
    let _ = std::fs::remove_file(with);
    let _ = std::fs::remove_file(without);
}

/// HEIC path: the app decoded the pixels and read EXIF through the platform, then passes
/// the focal length and the time in the options.
#[test]
fn frame_entry_takes_the_callers_focal_length_first() {
    let frame = Frame {
        width: W,
        height: H,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(field()),
    };
    let p = pool();
    let mut o = base();
    o.focal_length_35mm = Some(FOCAL_35MM as f32);
    o.observation_unix_ms = Some(TAKEN_UTC_MS);
    let r = p.solve_auto(&frame, &o, &aspect_ladder(W, H)).unwrap();
    assert_solved_first(&r);
    assert_eq!(r.outcome.observation_unix_ms, Some(TAKEN_UTC_MS));

    // The single-database ladder honors it too
    let solver = Solver::from_file(&synth::test_db_file("unisolver_15_40_exif.db")).unwrap();
    let (out, attempts) = solver
        .solve_with_fov_presets(&frame, &o, &aspect_ladder(W, H))
        .unwrap();
    assert!(matches!(out.status, SolveStatus::Ok));
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    assert_eq!(out.observation_unix_ms, Some(TAKEN_UTC_MS));
}
