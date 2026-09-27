//! unisolver-core: FFI-agnostic facade over tetra3 (spec §5.2 guarantee #1 —
//! this crate must never import FRB or any binding-framework types).
pub mod aberration;
pub mod annotate;
pub mod attribution;
mod bands;
pub mod calibrate;
pub mod camera;
pub mod coords;
pub mod dso;
pub mod ephemeris;
pub mod error;
pub mod frame;
#[cfg(feature = "imageio")]
pub mod imageio;
mod names;
pub mod names_pack;
pub mod outcome;
pub mod pool;
mod quat;
#[cfg(feature = "satellites")]
pub mod satellites;
mod search;
pub mod solver;

pub use aberration::days_since_j2000;
pub use annotate::{
    AnnotateOptions, Annotations, Annotator, DsoAnnotation, DsoOutline, LanguageCode,
    LayerAvailability, NamedStarAnnotation, OutlineContour, SatelliteAnnotation, StarAnnotation,
};
pub use attribution::{data_attributions, DataAttribution};
pub use calibrate::{CalibModel, CalibrationReport, CalibrationSession};
pub use camera::{CameraParams, DistortionParams};
pub use coords::{center_to_topleft, topleft_to_center};
pub use ephemeris::Observer;
pub use error::{CoreError, Result};
pub use frame::{Frame, PixelData};
pub use names_pack::NamesPack;
pub use outcome::{CentroidOut, MatchOut, SolveOutcome, SolveStatus, SolvedGeometry, Timing, Wcs};
pub use pool::{PoolAttempt, PoolOutcome, SolverPool, TierInfo};
#[cfg(feature = "imageio")]
pub use solver::presets_with_hints;
pub use solver::{aspect_ladder, focal_35mm_hint, ladder_after_hints};
pub use solver::{
    DbProperties, ExtractionOptions, ExtractionProfile, FovAttempt, FovPreset, SolveOptions, Solver,
};

#[doc(hidden)]
pub mod test_support {
    pub fn wxyz(q: &numeris::Quaternion<f32>) -> [f32; 4] {
        crate::quat::quat_to_wxyz(q)
    }
}
