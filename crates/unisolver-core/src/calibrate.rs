use crate::{camera::CameraParams, CoreError, Frame, Result, SolveOptions, SolveOutcome, Solver};
use serde::{Deserialize, Serialize};
use tetra3::{calibrate_camera, CalibrateConfig, Centroid, DistortionModelType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalibModel {
    Radial,
    Polynomial { order: u8 },
}

pub struct CalibrationSession {
    /// The session owns its own Solver (same `Arc<SolverDatabase>`, its own thread
    /// pool): FFI objects have independent lifetimes, so it cannot borrow `&Solver`.
    solver: Solver,
    frames: Vec<(tetra3::Solution, Vec<Centroid>)>,
    dims: Option<(u32, u32)>,
}

impl Solver {
    pub fn new_calibration_session(&self) -> Result<CalibrationSession> {
        Ok(CalibrationSession {
            solver: Solver::from_db(self.db().clone())?,
            frames: Vec::new(),
            dims: None,
        })
    }
}

impl CalibrationSession {
    /// Solves with `opts`. A solved frame adds (Solution, centre-origin centroids) to
    /// the session; a failed frame only returns its outcome. All frames must share
    /// the first frame's size.
    pub fn add_frame(&mut self, frame: &Frame, opts: &SolveOptions) -> Result<SolveOutcome> {
        if let Some((w, h)) = self.dims {
            if (frame.width, frame.height) != (w, h) {
                return Err(CoreError::InvalidInput(format!(
                    "calibration frames must share dimensions: {w}x{h} vs {}x{}",
                    frame.width, frame.height
                )));
            }
        }
        let (out, raw) = self.solver.solve_inner(frame, opts)?;
        if let Some(pair) = raw {
            self.dims.get_or_insert((frame.width, frame.height));
            self.frames.push(pair);
        }
        Ok(out)
    }

    pub fn count(&self) -> usize {
        self.frames.len()
    }

    pub fn fit(&self, model: CalibModel) -> Result<CalibrationReport> {
        let Some((w, h)) = self.dims else {
            return Err(CoreError::InvalidInput(
                "no successful frames in session".into(),
            ));
        };
        let model_t3 = match model {
            CalibModel::Radial => DistortionModelType::Radial,
            CalibModel::Polynomial { order } => {
                if !(2..=6).contains(&order) {
                    return Err(CoreError::InvalidInput(format!(
                        "poly order must be 2..=6, got {order}"
                    )));
                }
                DistortionModelType::Polynomial {
                    order: order as u32,
                }
            }
        };
        let solve_results: Vec<tetra3::SolveResult> =
            self.frames.iter().map(|(s, _)| Ok(s.clone())).collect();
        let sr_refs: Vec<&tetra3::SolveResult> = solve_results.iter().collect();
        let cent_refs: Vec<&[Centroid]> = self.frames.iter().map(|(_, c)| c.as_slice()).collect();
        let r = calibrate_camera(
            &sr_refs,
            &cent_refs,
            self.solver.db(),
            w,
            h,
            &CalibrateConfig {
                model: model_t3,
                ..Default::default()
            },
        )?;
        Ok(CalibrationReport {
            camera: CameraParams::from_tetra3(&r.camera_model),
            rmse_before_px: r.rmse_before_px,
            rmse_after_px: r.rmse_after_px,
            n_inliers: r.n_inliers,
            n_outliers: r.n_outliers,
            frames_used: self.frames.len(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub camera: CameraParams,
    pub rmse_before_px: f64,
    pub rmse_after_px: f64,
    pub n_inliers: usize,
    pub n_outliers: usize,
    pub frames_used: usize,
}
