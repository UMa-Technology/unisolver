//! EXIF on the private corpus of real phone photos (which carry none): each frame is solved
//! plainly, then again as a copy whose EXIF states the FOV it solved at, rounded to a whole
//! 35 mm focal length as phones write it. The copy must solve on its first rung at the same
//! pointing, within what two rungs give on a distorting wide lens (1% of the field; measured
//! up to 0.49%, and the golden regression allows 0.3° at 73°).
//!
//! Edge frames solved plainly only when a rung was revisited (the all-centroid probe; one
//! needs 65–120 ms of it on a desktop depending on the estimate). With EXIF that rung is
//! informed and gets the longer probe, so the copy must solve too, if not on its first attempt. Without the
//! corpus the test prints a skipped line and passes.
mod common;

use common::{bundled_w_db, repo_root};
use unisolver_core::*;
use unisolver_synth::exif::{jpeg_with_exif, ExifFields};

/// 35 mm-equivalent focal length whose CIPA diagonal FOV matches a horizontal FOV on w × h
fn focal_35mm(h_fov_deg: f32, w: u32, h: u32) -> u16 {
    let diag = ((w * w + h * h) as f64).sqrt();
    let tan_half_diag = (h_fov_deg as f64 / 2.0).to_radians().tan() * diag / w as f64;
    (21.633 / tan_half_diag).round() as u16
}

#[test]
fn exif_focal_length_solves_real_photos_on_the_first_rung() {
    let dir = repo_root().join("testdata/private/mobile");
    if !dir.exists() {
        eprintln!("skipped: needs the private corpus of real photos");
        return;
    }
    let mut files: Vec<_> = walk(&dir)
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
        })
        .collect();
    files.sort();
    let mut pool = SolverPool::new().unwrap();
    pool.register(bundled_w_db().to_str().unwrap()).unwrap();
    let base = SolveOptions::new(70.0);
    let tmp = std::env::temp_dir().join(format!("unisolver_exif_local_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();

    let (mut solved, mut first_rung) = (0, 0);
    let (mut attempts_plain, mut attempts_exif, mut ms_plain, mut ms_exif) = (0, 0, 0.0f32, 0.0f32);
    let mut worst = 0.0f64;
    let mut edges = Vec::new();
    for f in &files {
        let plain = pool
            .solve_image_file_auto(f.to_str().unwrap(), &base)
            .unwrap();
        let Some(g) = plain.outcome.solution.as_ref() else {
            continue;
        };
        solved += 1;
        let (w, h) = (g.wcs.width, g.wcs.height);
        let exif = ExifFields {
            focal_35mm: Some(focal_35mm(g.fov_deg, w, h)),
            ..Default::default()
        };
        let copy = tmp.join(f.file_name().unwrap());
        std::fs::write(&copy, jpeg_with_exif(&std::fs::read(f).unwrap(), &exif)).unwrap();
        let hinted = pool
            .solve_image_file_auto(copy.to_str().unwrap(), &base)
            .unwrap();
        let name = f.strip_prefix(&dir).unwrap().display().to_string();

        let last = plain.attempts.last().unwrap();
        let revisited = plain.attempts[..plain.attempts.len() - 1]
            .iter()
            .any(|a| a.fov_deg == last.fov_deg);
        if revisited {
            assert!(
                hinted.outcome.solution.is_some(),
                "{name}: solved plainly on attempt {}, not with EXIF: {:?}",
                plain.attempts.len(),
                hinted.attempts
            );
            edges.push(format!(
                "{name}: plain solved on attempt {}, with EXIF {:?} after {}",
                plain.attempts.len(),
                hinted.outcome.status,
                hinted.attempts.len()
            ));
            continue;
        }
        let h2 = hinted.outcome.solution.as_ref().unwrap_or_else(|| {
            panic!(
                "{name}: solved plainly ({:?}), not with EXIF: {:?}",
                plain.attempts, hinted.attempts
            )
        });
        let apart = angular_deg(g.ra_deg, g.dec_deg, h2.ra_deg, h2.dec_deg) / g.fov_deg as f64;
        assert!(
            apart < 0.01,
            "{name}: {:.3}% of the field apart",
            apart * 100.0
        );
        worst = worst.max(apart);
        assert_eq!(hinted.attempts.len(), 1, "{name}: {:?}", hinted.attempts);
        first_rung += 1;
        attempts_plain += plain.attempts.len();
        attempts_exif += hinted.attempts.len();
        ms_plain += plain.outcome.timing.total_ms;
        ms_exif += hinted.outcome.timing.total_ms;
    }
    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "EXIF on {} photos: {solved} solved plainly; {first_rung} on the EXIF rung ({attempts_exif} \
         attempts, {ms_exif:.0} ms in all, versus {attempts_plain} attempts and {ms_plain:.0} ms \
         plainly); pointings at most {:.3}% of the field apart",
        files.len(),
        worst * 100.0
    );
    for e in &edges {
        eprintln!("  edge: {e}");
    }
    assert!(solved >= 43, "{solved} solved");
    assert!(edges.len() <= 2, "{edges:?}");
}

fn angular_deg(ra1: f64, dec1: f64, ra2: f64, dec2: f64) -> f64 {
    let (r1, d1, r2, d2) = (
        ra1.to_radians(),
        dec1.to_radians(),
        ra2.to_radians(),
        dec2.to_radians(),
    );
    let c = d1.sin() * d2.sin() + d1.cos() * d2.cos() * (r1 - r2).cos();
    c.clamp(-1.0, 1.0).acos().to_degrees()
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
