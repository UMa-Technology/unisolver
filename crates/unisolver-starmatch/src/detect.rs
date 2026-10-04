// Derived from seiza v0.19.2 (https://github.com/theatrus/seiza), Copyright Yann Ramin,
// Apache-2.0. Changed for unisolver: only the detection result type is kept.

/// A detected star in pixel coordinates (0-indexed, sub-pixel centroid).
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedStar {
    pub x: f64,
    pub y: f64,
    /// Background-subtracted integrated flux
    pub flux: f64,
    /// Peak background-subtracted pixel value
    pub peak: f32,
    /// Component area in pixels
    pub area: u32,
}
