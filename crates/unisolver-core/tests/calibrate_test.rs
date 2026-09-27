use unisolver_core::*;
use unisolver_synth as synth;

#[test]
fn radial_calibration_recovers_k1_and_improves_rmse() {
    let solver = Solver::from_file(&synth::test_db_file("unisolver_core_test.db")).unwrap();
    let mut session = solver.new_calibration_session().unwrap();

    // Real distortion: k1 = -2e-8 (≈1% barrel at r ≈ 700 px on a 1200 px frame)
    let k1_true = -2.0e-8;
    let dist = tetra3::RadialDistortion {
        k1: k1_true,
        k2: 0.0,
        k3: 0.0,
        p1: 0.0,
        p2: 0.0,
        center: [0.0, 0.0],
    };
    let (w, h) = (1200u32, 900u32);
    for (i, (ra, dec)) in [(50.0, 20.0), (140.0, -10.0), (230.0, 45.0)]
        .iter()
        .enumerate()
    {
        let q = synth::look_at(*ra, *dec, 25.0 * i as f64);
        // "A frame from a distorted camera": push each ideal centroid through the forward
        // distortion, then draw a 3x3 point source
        let cents = synth::ideal_centroids(
            synth::test_db().star_catalog.stars(),
            &q,
            25.0,
            w,
            h,
            Some(7.5),
            0.0,
            i as u64,
        );
        let mut img = vec![100.0f32; (w * h) as usize];
        for c in &cents {
            let (xd, yd) = dist.distort(c.x as f64, c.y as f64); // centre-origin distortion
            let (xt, yt) = unisolver_core::center_to_topleft(xd, yd, w, h);
            let (ix, iy) = (xt.round() as i64, yt.round() as i64);
            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let (xx, yy) = (ix + dx, iy + dy);
                    if xx >= 0 && yy >= 0 && xx < w as i64 && yy < h as i64 {
                        let g = if dx == 0 && dy == 0 { 400.0 } else { 80.0 };
                        img[(yy as u32 * w + xx as u32) as usize] +=
                            g * c.mass.unwrap_or(1.0).min(50.0);
                    }
                }
            }
        }
        let frame = Frame {
            width: w,
            height: h,
            row_stride_bytes: None,
            pixels: PixelData::LumaF32(img),
        };
        let mut o = SolveOptions::new(25.0);
        o.fov_max_error_deg = Some(3.0);
        let out = session.add_frame(&frame, &o).unwrap();
        assert!(
            matches!(out.status, SolveStatus::Ok),
            "frame {i} status {:?}",
            out.status
        );
    }
    assert_eq!(session.count(), 3);
    let rep = session.fit(CalibModel::Radial).unwrap();
    assert!(
        rep.rmse_after_px < rep.rmse_before_px,
        "after {} !< before {}",
        rep.rmse_after_px,
        rep.rmse_before_px
    );
    match rep.camera.distortion {
        DistortionParams::Radial { k1, .. } => {
            assert!(k1 < 0.0, "k1 sign: {k1}");
            assert!(
                (k1 - k1_true).abs() < k1_true.abs(),
                "k1={k1} vs true {k1_true}"
            );
        }
        ref d => panic!("expected radial, got {d:?}"),
    }
}
