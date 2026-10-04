//! seiza's linear TAN solution (CRVAL, CRPIX, CD in degrees per pixel) as unisolver's
//! [`Wcs`], whose transforms run through a pinhole camera, a roll and a tangent point
//! (tetra3's model).
use crate::camera::{CameraParams, DistortionParams};
use crate::outcome::Wcs;

/// Converts a linear seiza WCS on a `width`×`height` frame. Pixel coordinates are top-left
/// origin, the same ones the centroids handed to seiza used.
///
/// tetra3's model is CD = (1/f)·[[p·cos θ, −sin θ], [p·sin θ, cos θ]] (p = −1 when mirrored).
/// An affine CD is projected onto the nearest such similarity: parity from the sign of the
/// determinant, f from its magnitude, θ from both columns.
#[allow(dead_code)] // used by the engine
pub(crate) fn wcs_from_seiza(sw: &seiza::Wcs, width: u32, height: u32) -> Wcs {
    let cd = [
        [sw.cd[0][0].to_radians(), sw.cd[0][1].to_radians()],
        [sw.cd[1][0].to_radians(), sw.cd[1][1].to_radians()],
    ];
    let det = cd[0][0] * cd[1][1] - cd[0][1] * cd[1][0];
    let parity_flip = det < 0.0;
    let p = if parity_flip { -1.0 } else { 1.0 };
    let focal_length_px = 1.0 / det.abs().sqrt();
    let theta_rad = (p * cd[1][0] - cd[0][1]).atan2(p * cd[0][0] + cd[1][1]);
    Wcs {
        width,
        height,
        cd,
        crval_deg: [sw.crval.0.rem_euclid(360.0), sw.crval.1],
        theta_rad,
        camera: CameraParams {
            focal_length_px,
            principal_point: sw.crpix,
            parity_flip,
            distortion: DistortionParams::None,
        },
    }
}

/// ICRS→camera attitude of a WCS as tetra3's `qicrs2cam` [w, x, y, z]: camera +Z at the
/// tangent point, +X and +Y the camera tangent-plane axes turned by the roll. Parity lives in
/// the camera model, not in the attitude.
#[allow(dead_code)] // used by the engine
pub(crate) fn attitude_wxyz(wcs: &Wcs) -> [f32; 4] {
    let (ra, dec) = (wcs.crval_deg[0].to_radians(), wcs.crval_deg[1].to_radians());
    let z = [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()];
    let east = [-ra.sin(), ra.cos(), 0.0];
    let north = [-dec.sin() * ra.cos(), -dec.sin() * ra.sin(), dec.cos()];
    let (s, c) = wcs.theta_rad.sin_cos();
    let x: [f64; 3] = std::array::from_fn(|i| c * east[i] + s * north[i]);
    let y: [f64; 3] = std::array::from_fn(|i| -s * east[i] + c * north[i]);
    let f = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32];
    let rot = numeris::Matrix3::new([f(x), f(y), f(z)]);
    crate::quat::quat_to_wxyz(&numeris::Quaternion::from_rotation_matrix(&rot))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sep_arcsec(a: (f64, f64), b: (f64, f64)) -> f64 {
        let (r1, d1, r2, d2) = (
            a.0.to_radians(),
            a.1.to_radians(),
            b.0.to_radians(),
            b.1.to_radians(),
        );
        let h =
            ((d2 - d1) / 2.0).sin().powi(2) + d1.cos() * d2.cos() * ((r2 - r1) / 2.0).sin().powi(2);
        (2.0 * h.sqrt().asin()).to_degrees() * 3600.0
    }

    #[test]
    fn converted_wcs_maps_pixels_like_seiza() {
        let (w, h) = (3000u32, 2000u32);
        let cases = [
            ((10.0, 20.0), 1.7, 0.0, false),
            ((212.4, -35.7), 6.0, 74.0, false),
            ((359.95, 80.0), 0.5, 200.0, true),
            ((0.02, -60.0), 2.0, 315.0, true),
        ];
        for (center, scale, rot, flipped) in cases {
            let sw = seiza::Wcs::from_center_scale_rotation(
                center,
                (1499.5, 999.5),
                scale,
                rot,
                flipped,
            );
            let ours = wcs_from_seiza(&sw, w, h);
            assert_eq!(ours.camera.parity_flip, flipped);
            assert!((ours.scale_arcsec_per_px() - scale).abs() < 1e-9 * scale.max(1.0));
            for &(x, y) in &[
                (0.0, 0.0),
                (2999.0, 0.0),
                (1499.5, 999.5),
                (100.0, 1900.0),
                (2900.0, 1800.0),
            ] {
                let a = sw.pixel_to_world(x, y);
                let b = ours.pixel_to_world(x, y);
                assert!(
                    sep_arcsec(a, b) < 1e-3,
                    "{center:?} {rot} {flipped} at ({x},{y}): {a:?} vs {b:?}"
                );
                let (px, py) = ours.world_to_pixel(a.0, a.1).unwrap();
                assert!((px - x).abs() < 1e-3 && (py - y).abs() < 1e-3);
            }
        }
    }

    /// The attitude follows tetra3's convention: on a tetra3 solve of a synthetic frame it
    /// agrees with the truth (as the SVD quaternion does, within 6′)
    #[test]
    fn attitude_matches_a_tetra3_solve() {
        use unisolver_synth as synth;
        let solver =
            crate::Solver::from_file(&synth::test_db_file("narrow_geometry_test.db")).unwrap();
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
        let frame = crate::Frame {
            width: 1024,
            height: 768,
            row_stride_bytes: None,
            pixels: crate::PixelData::LumaF32(img),
        };
        let sol = solver
            .solve(&frame, &crate::SolveOptions::new(20.0))
            .unwrap()
            .solution
            .expect("synthetic frame solves");
        let ours = attitude_wxyz(&sol.wcs);
        let truth = crate::test_support::wxyz(&q);
        let dot: f32 = (0..4)
            .map(|i| ours[i] * truth[i])
            .sum::<f32>()
            .abs()
            .min(1.0);
        let angle_arcmin = 2.0 * dot.acos().to_degrees() * 60.0;
        assert!(angle_arcmin < 6.0, "attitude off by {angle_arcmin}′");
    }
}
