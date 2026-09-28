use unisolver_core::*;
use unisolver_synth as synth;

fn frame_from(img: Vec<f32>, w: u32, h: u32) -> Frame {
    Frame {
        width: w,
        height: h,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(img),
    }
}

fn write_test_db() -> String {
    synth::test_db_file("unisolver_core_test.db")
}

#[test]
fn blind_solve_end_to_end() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(120.0, 40.0, 15.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        5,
    );
    let out = solver
        .solve(&frame_from(img, 1024, 768), &SolveOptions::new(20.0))
        .unwrap();
    assert!(
        matches!(out.status, SolveStatus::Ok),
        "status={:?}",
        out.status
    );
    let sol = out.solution.expect("solution");
    // Boresight is correct (< 0.05°, from the WCS)
    assert!(
        (sol.ra_deg - 120.0).abs() * 40.0f64.to_radians().cos() < 0.05
            && (sol.dec_deg - 40.0).abs() < 0.05,
        "ra/dec=({}, {})",
        sol.ra_deg,
        sol.dec_deg
    );
    // WCS round trip (top-left origin)
    let (ra, dec) = sol.wcs.pixel_to_world(512.0, 384.0);
    let (px, py) = sol.wcs.world_to_pixel(ra, dec).unwrap();
    assert!((px - 512.0).abs() < 0.2 && (py - 384.0).abs() < 0.2);
    // Centroid and match output is top-left origin and in bounds
    assert!(!out.centroids.is_empty() && !sol.matched.is_empty());
    for c in &out.centroids {
        assert!(c.x >= 0.0 && c.x < 1024.0 && c.y >= 0.0 && c.y < 768.0);
    }
    // The quaternion (SVD attitude) is within 6′ of truth (upstream's WCS refinement does not write it back)
    let qq = numeris::Quaternion::new(
        sol.quat_icrs2cam_wxyz[0],
        sol.quat_icrs2cam_wxyz[1],
        sol.quat_icrs2cam_wxyz[2],
        sol.quat_icrs2cam_wxyz[3],
    );
    assert!(synth::boresight_err_arcmin(&qq, &q) < 6.0);
}

#[test]
fn invalid_fov_is_err() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let img = vec![0.0f32; 64 * 64];
    let r = solver.solve(&frame_from(img, 64, 64), &SolveOptions::new(f32::NAN));
    assert!(matches!(r, Err(CoreError::InvalidInput(_))));
}

#[test]
fn tracking_mode_with_hint() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(300.0, -20.0, 5.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        11,
    );
    // Blind-solve first and use the attitude as the next frame's hint (fast extraction path)
    let blind = solver
        .solve(
            &frame_from(img.clone(), 1024, 768),
            &SolveOptions::new(20.0),
        )
        .unwrap();
    let g = blind.solution.expect("blind solution");

    let mut tracked = SolveOptions::new(20.0);
    tracked.extraction = ExtractionProfile::Custom(ExtractionOptions::Fast {
        sigma_threshold: 5.0,
        max_centroids: 60,
    });
    tracked.attitude_hint = Some(g.quat_icrs2cam_wxyz);
    tracked.hint_uncertainty_deg = 2.0;
    let out = solver
        .solve(&frame_from(img.clone(), 1024, 768), &tracked)
        .unwrap();
    assert!(
        matches!(out.status, SolveStatus::Ok),
        "tracked status={:?}",
        out.status
    );
    // Tracking should be at least as fast as a blind solve (only function and pointing are asserted)
    let ts = out.solution.unwrap();
    assert!(
        (ts.ra_deg - 300.0).abs() * 20.0f64.to_radians().cos() < 0.1
            && (ts.dec_deg + 20.0).abs() < 0.1,
        "tracked ra/dec=({}, {})",
        ts.ra_deg,
        ts.dec_deg
    );

    // strict_hint with a wildly wrong hint (boresight reversed) → no fallback, fails
    let mut bad = SolveOptions::new(20.0);
    bad.attitude_hint = Some({
        let q0 = numeris::Quaternion::new(
            g.quat_icrs2cam_wxyz[0],
            g.quat_icrs2cam_wxyz[1],
            g.quat_icrs2cam_wxyz[2],
            g.quat_icrs2cam_wxyz[3],
        );
        let flip = numeris::Quaternion::new(0.0, 1.0, 0.0, 0.0); // 180° about x
        unisolver_core::test_support::wxyz(&(flip * q0))
    });
    bad.strict_hint = true;
    bad.hint_uncertainty_deg = 1.0;
    let out = solver.solve(&frame_from(img, 1024, 768), &bad).unwrap();
    assert!(
        !matches!(out.status, SolveStatus::Ok),
        "strict wrong hint must fail"
    );
}

#[test]
fn fov_presets_fall_through_to_correct_scale() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(45.0, 25.0, 0.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        9,
    );
    let presets = [
        FovPreset {
            fov_deg: 38.0,
            max_error_deg: 2.0,
        }, // wrong rung
        FovPreset {
            fov_deg: 20.0,
            max_error_deg: 4.0,
        }, // right rung
    ];
    let (out, attempts) = solver
        .solve_with_fov_presets(
            &frame_from(img, 1024, 768),
            &SolveOptions::new(20.0),
            &presets,
        )
        .unwrap();
    assert!(matches!(out.status, SolveStatus::Ok));
    assert_eq!(attempts.len(), 2);
    assert!(
        !matches!(attempts[0].status, SolveStatus::Ok),
        "first preset should miss"
    );
    assert!((out.solution.unwrap().fov_deg - 20.0).abs() < 2.0);
}

#[test]
fn alternate_profile_retry_rescues_clean_sensor_image() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(150.0, 5.0, 40.0);
    // Hand-built "clean sensor" frame: noise σ=2; 3 bright stars peak at 60, the rest at 7
    // (3.5σ raw). Upstream's matched filter (1.5σ Gaussian) gains about 2× SNR, putting the
    // faint stars near 7σ: below the PhoneJpeg threshold (σ=10 → fewer than 4 usable → fails)
    // and above CleanSensor's (σ=5 → all visible → solves).
    let (w, h) = (1024u32, 768u32);
    let cents = synth::ideal_centroids(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        w,
        h,
        Some(6.0),
        0.0,
        0,
    );
    assert!(cents.len() >= 20, "need enough stars, got {}", cents.len());
    let mut img = vec![50.0f32; (w * h) as usize];
    for (i, c) in cents.iter().take(40).enumerate() {
        let peak = if i < 3 { 60.0 } else { 7.0 };
        let (xt, yt) = unisolver_core::center_to_topleft(c.x as f64, c.y as f64, w, h);
        let (ix, iy) = (xt.round() as i64, yt.round() as i64);
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                let (xx, yy) = (ix + dx, iy + dy);
                if xx >= 0 && yy >= 0 && xx < w as i64 && yy < h as i64 {
                    let g = if dx == 0 && dy == 0 {
                        peak
                    } else {
                        peak * 0.25
                    };
                    img[(yy as u32 * w + xx as u32) as usize] += g;
                }
            }
        }
    }
    // Deterministic Gaussian noise
    use rand_like::*;
    mod rand_like {
        pub struct Lcg(pub u64);
        impl Lcg {
            pub fn next_f32(&mut self) -> f32 {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((self.0 >> 33) as f32 / (1u64 << 31) as f32) - 1.0
            }
        }
    }
    let mut rng = Lcg(42);
    for p in &mut img {
        // Approximate Gaussian from a sum of 3 uniforms (enough for threshold statistics)
        *p += 2.0 * (rng.next_f32() + rng.next_f32() + rng.next_f32()) / 1.73;
    }

    let frame = Frame {
        width: w,
        height: h,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(img),
    };
    // Default PhoneJpeg with retry on (the default)
    let mut opts = SolveOptions::new(20.0);
    opts.fov_max_error_deg = Some(3.0);
    opts.timeout_ms = Some(15_000);
    let out = solver.solve(&frame, &opts).unwrap();
    assert!(
        matches!(out.status, SolveStatus::Ok),
        "expected rescue via alternate profile, got {:?} with {} centroids",
        out.status,
        out.centroids.len()
    );
    assert!(
        out.extraction_retried,
        "must have gone through profile retry"
    );

    // Retry off → fails on PhoneJpeg (proving the success came from the retry)
    let mut no_retry = opts.clone();
    no_retry.retry_alternate_profile = false;
    let out2 = solver.solve(&frame, &no_retry).unwrap();
    assert!(
        !matches!(out2.status, SolveStatus::Ok),
        "without retry PhoneJpeg profile should fail on this image"
    );
    assert!(!out2.extraction_retried);
}

#[test]
fn lying_header_hint_fails_but_ladder_recovers() {
    // A true 20° synthetic frame with a fake "header says 45°" hint: the hint rung must fail and the ladder must recover
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(200.0, -30.0, 0.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        3,
    );
    let lying_hint = FovPreset {
        fov_deg: 45.0,
        max_error_deg: 6.75,
    };
    let honest_ladder = [
        FovPreset {
            fov_deg: 42.0,
            max_error_deg: 7.0,
        },
        FovPreset {
            fov_deg: 20.0,
            max_error_deg: 5.0,
        },
    ];
    let presets: Vec<_> = std::iter::once(lying_hint).chain(honest_ladder).collect();
    let mut base = SolveOptions::new(20.0);
    base.extraction = ExtractionProfile::CleanSensor;
    let (out, attempts) = solver
        .solve_with_fov_presets(&frame_from(img, 1024, 768), &base, &presets)
        .unwrap();
    assert!(matches!(out.status, SolveStatus::Ok));
    assert!(
        !matches!(attempts[0].status, SolveStatus::Ok),
        "a lying hint rung must fail"
    );
    assert!(
        (out.solution.unwrap().fov_deg - 20.0).abs() < 2.0,
        "the ladder recovers the truth"
    );
}

#[test]
fn aspect_ladder_is_shared_single_source() {
    assert_eq!(aspect_ladder(1920, 1080)[0].fov_deg, 70.0);
    assert_eq!(aspect_ladder(720, 1280)[0].fov_deg, 46.0);
}

#[test]
fn auto_profile_sigma10_first_with_fallback() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(60.0, -10.0, 0.0);
    // Clean frame: Auto should take the σ5 route (solvable)
    let clean = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        21,
    );
    let mut o = SolveOptions::new(20.0);
    o.extraction = ExtractionProfile::Auto;
    o.fov_max_error_deg = Some(3.0);
    let out = solver
        .solve(
            &Frame {
                width: 1024,
                height: 768,
                row_stride_bytes: None,
                pixels: PixelData::LumaF32(clean),
            },
            &o,
        )
        .unwrap();
    assert!(matches!(out.status, SolveStatus::Ok), "{:?}", out.status);

    // Noise-storm frame (simulated compression noise plus real stars): Auto should switch to σ10 and still solve
    let mut noisy = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams {
            noise_sigma: 1.0,
            flux_mag8: 2000.0,
            ..Default::default()
        },
        22,
    );
    // Inject many 6σ single-pixel spikes (a morphological stand-in for JPEG noise)
    let mut lcg = 123456789u64;
    for _ in 0..40_000 {
        lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = (lcg >> 20) as usize % noisy.len();
        noisy[idx] += 7.0;
    }
    let out = solver
        .solve(
            &Frame {
                width: 1024,
                height: 768,
                row_stride_bytes: None,
                pixels: PixelData::LumaF32(noisy),
            },
            &o,
        )
        .unwrap();
    assert!(
        matches!(out.status, SolveStatus::Ok),
        "noisy auto: {:?}",
        out.status
    );
}

#[test]
fn presets_outside_db_range_are_clipped() {
    // test_db covers 15–40°: rungs outside it (each burns a full timeout on a deep database) must be clamped
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(45.0, 25.0, 0.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        9,
    );
    let presets = [
        FovPreset {
            fov_deg: 100.0,
            max_error_deg: 10.0,
        }, // > 40×1.25, clamped
        FovPreset {
            fov_deg: 2.0,
            max_error_deg: 0.5,
        }, // < 15×0.8, clamped
        FovPreset {
            fov_deg: 20.0,
            max_error_deg: 4.0,
        },
    ];
    let (out, attempts) = solver
        .solve_with_fov_presets(
            &frame_from(img, 1024, 768),
            &SolveOptions::new(20.0),
            &presets,
        )
        .unwrap();
    assert!(matches!(out.status, SolveStatus::Ok));
    assert_eq!(
        attempts.len(),
        1,
        "out-of-range presets must not be attempted"
    );
    assert!((attempts[0].fov_deg - 20.0).abs() < 0.1);
}

#[test]
fn all_presets_clipped_falls_back_to_db_range_ladder() {
    // The prior ladder does not match the database at all (phone rungs vs a deep database):
    // sweep the database range instead. 15–40° ladder = 40, 30, 22.5, 16.9, 15; 22.5 ± 20% covers 20°.
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(45.0, 25.0, 0.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        9,
    );
    let presets = [FovPreset {
        fov_deg: 2.0,
        max_error_deg: 0.5,
    }];
    let (out, attempts) = solver
        .solve_with_fov_presets(
            &frame_from(img, 1024, 768),
            &SolveOptions::new(20.0),
            &presets,
        )
        .unwrap();
    assert!(
        matches!(out.status, SolveStatus::Ok),
        "db-range ladder should rescue: {:?}",
        attempts
    );
    assert!(
        (attempts[0].fov_deg - 40.0).abs() < 0.1,
        "ladder starts at db max"
    );
    let solved = out.solution.unwrap().fov_deg;
    assert!((solved - 20.0).abs() < 2.0, "solved fov {solved}");
}

#[test]
fn elongation_diagnostic_is_reported_and_solve_unchanged() {
    // Elongation is a **read-only diagnostic**: it neither filters nor changes the solve.
    // Locks two things: a synthetic round-star field measures close to 1, and the solve is unaffected.
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(45.0, 25.0, 0.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        9,
    );
    let out = solver
        .solve(&frame_from(img, 1024, 768), &SolveOptions::new(20.0))
        .unwrap();
    assert!(matches!(out.status, SolveStatus::Ok));
    let e = out
        .median_elongation
        .expect("the CCL path provides covariance");
    assert!(
        (1.0..1.6).contains(&e),
        "round synthetic stars should measure close to 1, got {e}"
    );
    // Per-centroid values are present too (for callers' own statistics)
    assert!(out.centroids.iter().all(|c| c.elongation.is_some()));
    assert!(out.centroids.iter().all(|c| c.elongation.unwrap() >= 1.0));
}

/// A field that is not in the database (a different random sky) fails through the staged
/// search: exactly the schedule's attempts, and `thorough` appends the exhaustive search.
#[test]
fn unmatched_field_fails_through_the_staged_schedule() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let foreign = synth::random_sky(20_000, 99, 3.0, 8.0);
    let q = synth::look_at(40.0, 20.0, 0.0);
    let img = synth::render(
        &foreign,
        &q,
        25.0,
        1024,
        768,
        &synth::RenderParams::default(),
        5,
    );
    let frame = frame_from(img, 1024, 768);
    let presets: Vec<FovPreset> = [36.0, 30.0, 25.0, 20.0, 16.0]
        .into_iter()
        .map(|fov_deg| FovPreset {
            fov_deg,
            max_error_deg: 2.0,
        })
        .collect();
    let mut o = SolveOptions::new(25.0);
    o.extraction = ExtractionProfile::CleanSensor;
    o.timeout_ms = Some(1000);

    let (out, attempts) = solver.solve_with_fov_presets(&frame, &o, &presets).unwrap();
    assert!(!matches!(out.status, SolveStatus::Ok));
    let fovs: Vec<f32> = attempts.iter().map(|a| a.fov_deg).collect();
    // quick sweep of the first three rungs, a deep probe of the first, then the rest
    assert_eq!(fovs, [36.0, 30.0, 25.0, 36.0, 20.0, 16.0]);

    o.thorough = true;
    let (_, attempts) = solver.solve_with_fov_presets(&frame, &o, &presets).unwrap();
    assert_eq!(
        attempts.len(),
        6 + 5,
        "thorough appends every rung once more"
    );
}

/// The observation time never moves the solution: the WCS stays in the J2000 catalog frame,
/// so catalog positions project onto the stars. (Feeding the time into the aberration
/// correction put every annotation layer up to 20″ off the stars: 10 px at 2″/px.) An
/// explicit observer velocity still asks for the physical pointing.
#[test]
fn observation_time_leaves_the_solution_in_the_catalog_frame() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(120.0, 40.0, 15.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        5,
    );
    let frame = frame_from(img, 1024, 768);
    let solve = |opts: SolveOptions| solver.solve(&frame, &opts).unwrap();
    let arcsec = |a: &SolvedGeometry, b: &SolvedGeometry| {
        let (a0, a1) = (a.ra_deg.to_radians(), a.dec_deg.to_radians());
        let (b0, b1) = (b.ra_deg.to_radians(), b.dec_deg.to_radians());
        let h =
            ((b1 - a1) / 2.0).sin().powi(2) + a1.cos() * b1.cos() * ((b0 - a0) / 2.0).sin().powi(2);
        (2.0 * h.sqrt().asin()).to_degrees() * 3600.0
    };
    let plain = solve(SolveOptions::new(20.0)).solution.expect("solution");

    let mut timed = SolveOptions::new(20.0);
    timed.observation_unix_ms = Some(1_788_614_467_378);
    let out = solve(timed);
    assert_eq!(out.observation_unix_ms, Some(1_788_614_467_378), "reported");
    let t = out.solution.expect("solution");
    assert!(arcsec(&plain, &t) < 0.01, "{}″", arcsec(&plain, &t));
    assert!((plain.roll_deg - t.roll_deg).abs() < 1e-6);

    // 30 km/s toward RA 90°: 48.5° from the boresight, so about 20.6″ × sin 48.5° = 15.4″
    let mut moving = SolveOptions::new(20.0);
    moving.observer_velocity_km_s = Some([0.0, 30.0, 0.0]);
    let m = solve(moving).solution.expect("solution");
    let shift = arcsec(&plain, &m);
    assert!(shift > 12.0 && shift < 19.0, "{shift}″");
}

/// A wide frame through a lens that bends the edges by ~6 px: the pinhole solve leaves that as
/// residual, the lens fit (on by default) takes it out and recovers the distortion. Off, with a
/// caller's camera, or in narrow fields the solve stays pinhole.
#[test]
fn wide_frames_fit_the_lens() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let q = synth::look_at(210.0, -20.0, 25.0);
    let render = |fov: f32, k1: f32| {
        let img = synth::render(
            synth::test_db().star_catalog.stars(),
            &q,
            fov,
            1024,
            768,
            &synth::RenderParams {
                k1,
                ..Default::default()
            },
            9,
        );
        frame_from(img, 1024, 768)
    };
    let k1 = 2.0e-8; // 640³ · 2e-8 ≈ 5 px at the corners
    let bent = render(35.0, k1);
    let solve = |f: &Frame, o: SolveOptions| solver.solve(f, &o).unwrap().solution.expect("solved");

    let fitted = solve(&bent, SolveOptions::new(35.0));
    let mut off = SolveOptions::new(35.0);
    off.fit_lens = false;
    let pinhole = solve(&bent, off);
    assert!(!pinhole.lens_fitted);
    assert!(fitted.lens_fitted, "{} matches", fitted.num_matches);
    assert!(
        fitted.rmse_arcsec < pinhole.rmse_arcsec * 0.6,
        "rmse {} vs pinhole {}",
        fitted.rmse_arcsec,
        pinhole.rmse_arcsec
    );
    match fitted.wcs.camera.distortion {
        DistortionParams::Radial { k1: got, k2, .. } => {
            assert!((got / k1 as f64 - 1.0).abs() < 0.3, "k1 {got:e}");
            assert_eq!(k2, 0.0);
        }
        ref d => panic!("{d:?}"),
    }

    // A caller's camera is kept as given
    let mut given = SolveOptions::new(35.0);
    given.camera = Some(CameraParams::from_horizontal_fov(35.0, 1024, 768).unwrap());
    assert!(!solve(&bent, given).lens_fitted);

    // Narrow fields are left pinhole (distortion there is below a pixel)
    assert!(!solve(&render(16.0, k1), SolveOptions::new(16.0)).lens_fitted);

    // A clean lens: whatever the fit decides, the pointing stays where it was
    let clean = render(35.0, 0.0);
    let a = solve(&clean, SolveOptions::new(35.0));
    let mut off = SolveOptions::new(35.0);
    off.fit_lens = false;
    let b = solve(&clean, off);
    let sep = ((a.ra_deg - b.ra_deg) * a.dec_deg.to_radians().cos()).hypot(a.dec_deg - b.dec_deg);
    assert!(sep * 3600.0 < 30.0, "{}″", sep * 3600.0);
}

/// A lens whose distortion changes sign (barrel in the middle, pincushion at the edges) needs
/// k2, which k1 alone cannot stand in for: the fit takes it and recovers both terms, the corners
/// included, although the pinhole solve could not match the stars there.
#[test]
fn wide_frames_fit_k2_when_the_lens_needs_it() {
    let solver = Solver::from_file(&write_test_db()).unwrap();
    let (k1, k2) = (-2.0e-8, 1.0e-13); // at the corners (r = 640) −5.2 + 10.7 px
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &synth::look_at(210.0, -20.0, 25.0),
        35.0,
        1024,
        768,
        &synth::RenderParams {
            k1: k1 as f32,
            k2: k2 as f32,
            ..Default::default()
        },
        9,
    );
    let g = solver
        .solve(&frame_from(img, 1024, 768), &SolveOptions::new(35.0))
        .unwrap()
        .solution
        .expect("solved");
    assert!(g.lens_fitted, "{} matches", g.num_matches);
    let DistortionParams::Radial {
        k1: got1, k2: got2, ..
    } = g.wcs.camera.distortion
    else {
        panic!("{:?}", g.wcs.camera.distortion);
    };
    assert!((got1 / k1 - 1.0).abs() < 0.1, "k1 {got1:e}");
    assert!((got2 / k2 - 1.0).abs() < 0.2, "k2 {got2:e}");
    let corner = |a: f64, b: f64| a * 640f64.powi(3) + b * 640f64.powi(5);
    assert!(
        (corner(got1, got2) - corner(k1, k2)).abs() < 0.5,
        "corner {:.2} px, rendered {:.2}",
        corner(got1, got2),
        corner(k1, k2)
    );
}
