use crate::{coords, CoreError, Result};
use serde::{Deserialize, Serialize};
use tetra3::{num_coeffs, CameraModel, Distortion, PolynomialDistortion, RadialDistortion};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DistortionParams {
    None,
    /// Brown-Conrady: radial k1..k3 plus tangential p1/p2, in pixel units (r in
    /// pixels, see vendored radial.rs). `center` is top-left origin; None = principal point.
    Radial {
        k1: f64,
        k2: f64,
        k3: f64,
        p1: f64,
        p2: f64,
        center: Option<(f64, f64)>,
    },
    Polynomial {
        order: u32,
        scale: f64,
        a_coeffs: Vec<f64>,
        b_coeffs: Vec<f64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraParams {
    pub focal_length_px: f64,
    /// Pixel coordinates, top-left origin
    pub principal_point: (f64, f64),
    pub parity_flip: bool,
    pub distortion: DistortionParams,
}

fn check_fov(fov_deg: f64) -> Result<()> {
    if !(fov_deg.is_finite() && fov_deg > 0.1 && fov_deg < 179.0) {
        return Err(CoreError::InvalidInput(format!(
            "fov_deg must be in (0.1, 179), got {fov_deg}"
        )));
    }
    Ok(())
}
fn check_dims(width: u32, height: u32) -> Result<()> {
    if width < 2 || height < 2 {
        return Err(CoreError::InvalidInput(format!(
            "image dims too small: {width}x{height}"
        )));
    }
    Ok(())
}

impl CameraParams {
    pub fn from_horizontal_fov(fov_deg: f64, width: u32, height: u32) -> Result<Self> {
        check_fov(fov_deg)?;
        check_dims(width, height)?;
        let f = (width as f64 / 2.0) / (fov_deg.to_radians() / 2.0).tan();
        Ok(Self {
            focal_length_px: f,
            principal_point: ((width as f64 - 1.0) / 2.0, (height as f64 - 1.0) / 2.0),
            parity_flip: false,
            distortion: DistortionParams::None,
        })
    }

    pub fn from_diagonal_fov(diag_fov_deg: f64, width: u32, height: u32) -> Result<Self> {
        check_fov(diag_fov_deg)?;
        check_dims(width, height)?;
        let diag_px = ((width as f64).powi(2) + (height as f64).powi(2)).sqrt();
        let t = (diag_fov_deg.to_radians() / 2.0).tan() * width as f64 / diag_px;
        Self::from_horizontal_fov(2.0 * t.atan().to_degrees(), width, height)
    }

    /// EXIF FocalLengthIn35mmFilm (CIPA diagonal equivalent; full-frame diagonal 43.266 mm)
    pub fn from_equivalent_focal_35mm(mm: f64, width: u32, height: u32) -> Result<Self> {
        if !(mm.is_finite() && mm > 1.0 && mm < 2000.0) {
            return Err(CoreError::InvalidInput(format!(
                "35mm focal out of range: {mm}"
            )));
        }
        Self::from_diagonal_fov(2.0 * (43.266 / 2.0 / mm).atan().to_degrees(), width, height)
    }

    /// Horizontal FOV in degrees implied by the focal length
    pub fn horizontal_fov_deg(&self, width: u32) -> f64 {
        2.0 * ((width as f64 / 2.0) / self.focal_length_px)
            .atan()
            .to_degrees()
    }

    pub fn validate(&self, width: u32, height: u32) -> Result<()> {
        check_dims(width, height)?;
        if !(self.focal_length_px.is_finite() && self.focal_length_px > 1.0) {
            return Err(CoreError::InvalidInput(format!(
                "bad focal_length_px {}",
                self.focal_length_px
            )));
        }
        let (px, py) = self.principal_point;
        if !(px.is_finite() && py.is_finite()) {
            return Err(CoreError::InvalidInput("principal_point not finite".into()));
        }
        match &self.distortion {
            DistortionParams::None => {}
            DistortionParams::Radial {
                k1,
                k2,
                k3,
                p1,
                p2,
                center,
            } => {
                for k in [k1, k2, k3, p1, p2] {
                    if !k.is_finite() {
                        return Err(CoreError::InvalidInput("radial k not finite".into()));
                    }
                }
                if let Some((cx, cy)) = center {
                    if !(cx.is_finite() && cy.is_finite()) {
                        return Err(CoreError::InvalidInput("radial center not finite".into()));
                    }
                }
            }
            DistortionParams::Polynomial {
                order,
                scale,
                a_coeffs,
                b_coeffs,
            } => {
                // Upstream PolynomialDistortion::new panics on a bad order or length; reject it first
                if !(2..=6).contains(order) {
                    return Err(CoreError::InvalidInput(format!(
                        "poly order must be 2..=6, got {order}"
                    )));
                }
                if !scale.is_finite() || *scale == 0.0 {
                    return Err(CoreError::InvalidInput("poly scale invalid".into()));
                }
                let n = num_coeffs(*order);
                if a_coeffs.len() != n || b_coeffs.len() != n {
                    return Err(CoreError::InvalidInput(format!(
                        "poly coeffs len mismatch: need {n}, got a={} b={}",
                        a_coeffs.len(),
                        b_coeffs.len()
                    )));
                }
                if a_coeffs.iter().chain(b_coeffs).any(|c| !c.is_finite()) {
                    return Err(CoreError::InvalidInput("poly coeff not finite".into()));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn to_tetra3(&self, width: u32, height: u32) -> Result<CameraModel> {
        self.validate(width, height)?;
        let crpix_arr = {
            let (cx, cy) = coords::topleft_to_center(
                self.principal_point.0,
                self.principal_point.1,
                width,
                height,
            );
            [cx, cy]
        };
        let distortion = match &self.distortion {
            DistortionParams::None => Distortion::None,
            DistortionParams::Radial {
                k1,
                k2,
                k3,
                p1,
                p2,
                center,
            } => {
                let c = center
                    .map(|(x, y)| coords::topleft_to_center(x, y, width, height))
                    .map(|(x, y)| [x, y])
                    .unwrap_or(crpix_arr);
                Distortion::Radial(RadialDistortion {
                    k1: *k1,
                    k2: *k2,
                    k3: *k3,
                    p1: *p1,
                    p2: *p2,
                    center: c,
                })
            }
            DistortionParams::Polynomial {
                order,
                scale,
                a_coeffs,
                b_coeffs,
            } => Distortion::Polynomial(PolynomialDistortion::new(
                *order,
                *scale,
                a_coeffs.clone(),
                b_coeffs.clone(),
            )),
        };
        Ok(CameraModel {
            focal_length_px: self.focal_length_px,
            image_width: width,
            image_height: height,
            crpix: crpix_arr,
            parity_flip: self.parity_flip,
            distortion,
        })
    }

    pub(crate) fn from_tetra3(m: &CameraModel) -> CameraParams {
        let pp = coords::center_to_topleft(m.crpix[0], m.crpix[1], m.image_width, m.image_height);
        let distortion = match &m.distortion {
            Distortion::None => DistortionParams::None,
            Distortion::Radial(r) => DistortionParams::Radial {
                k1: r.k1,
                k2: r.k2,
                k3: r.k3,
                p1: r.p1,
                p2: r.p2,
                center: Some(coords::center_to_topleft(
                    r.center[0],
                    r.center[1],
                    m.image_width,
                    m.image_height,
                )),
            },
            Distortion::Polynomial(p) => DistortionParams::Polynomial {
                order: p.order,
                scale: p.scale,
                a_coeffs: p.a_coeffs.clone(),
                b_coeffs: p.b_coeffs.clone(),
            },
        };
        CameraParams {
            focal_length_px: m.focal_length_px,
            principal_point: pp,
            parity_flip: m.parity_flip,
            distortion,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn horizontal_fov_roundtrip() {
        let c = CameraParams::from_horizontal_fov(73.7, 4032, 3024).unwrap();
        let m = c.to_tetra3(4032, 3024).unwrap();
        assert!((m.fov_deg() - 73.7).abs() < 0.01);
        assert_eq!(m.crpix, [0.0, 0.0]); // default principal point = image centre, so no offset
    }
    #[test]
    fn equivalent_35mm_uses_diagonal_equivalence() {
        // CIPA: the 35 mm equivalent is a diagonal equivalent. diag_fov = 2·atan(21.633/24);
        // at 4:3 the horizontal FOV = 2·atan(tan(diag/2)·w/diag_px) ≈ 71.6°
        let c = CameraParams::from_equivalent_focal_35mm(24.0, 4032, 3024).unwrap();
        let fov = c.to_tetra3(4032, 3024).unwrap().fov_deg();
        assert!((fov - 71.6).abs() < 0.3, "fov={fov}");
    }
    #[test]
    fn invalid_inputs_err_not_panic() {
        assert!(CameraParams::from_horizontal_fov(f64::NAN, 100, 100).is_err());
        assert!(CameraParams::from_horizontal_fov(0.0, 100, 100).is_err());
        assert!(CameraParams::from_horizontal_fov(180.0, 100, 100).is_err());
        // Coefficient count that does not match the order must be an Err (upstream new() panics)
        let bad = CameraParams {
            focal_length_px: 1000.0,
            principal_point: (49.5, 49.5),
            parity_flip: false,
            distortion: DistortionParams::Polynomial {
                order: 3,
                scale: 1.0,
                a_coeffs: vec![0.0; 2],
                b_coeffs: vec![0.0; 2],
            },
        };
        assert!(bad.validate(100, 100).is_err());
        // Order out of range (upstream calibrate accepts 2..=6 only)
        let bad2 = CameraParams {
            distortion: DistortionParams::Polynomial {
                order: 9,
                scale: 1.0,
                a_coeffs: vec![0.0; 55],
                b_coeffs: vec![0.0; 55],
            },
            ..bad
        };
        assert!(bad2.validate(100, 100).is_err());
    }
}
