/// tetra3 puts the centroid origin at the image centre ((W−1)/2, (H−1)/2), +X right,
/// +Y down (pixel-centre convention, vendored ccl.rs:194). The public API always
/// uses a top-left origin.
pub fn topleft_to_center(x: f64, y: f64, width: u32, height: u32) -> (f64, f64) {
    (
        x - (width as f64 - 1.0) / 2.0,
        y - (height as f64 - 1.0) / 2.0,
    )
}

pub fn center_to_topleft(x: f64, y: f64, width: u32, height: u32) -> (f64, f64) {
    (
        x + (width as f64 - 1.0) / 2.0,
        y + (height as f64 - 1.0) / 2.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn half_pixel_convention_matches_tetra3_ccl() {
        // tetra3 centre = ((W−1)/2, (H−1)/2) (ccl.rs:194). W=4: top-left pixel (0,0)
        // maps to (−1.5, −1.5) in centre-origin coordinates
        assert_eq!(topleft_to_center(0.0, 0.0, 4, 4), (-1.5, -1.5));
        assert_eq!(center_to_topleft(-1.5, -1.5, 4, 4), (0.0, 0.0));
        let (cx, cy) = topleft_to_center(511.5, 383.5, 1024, 768);
        assert_eq!((cx, cy), (0.0, 0.0));
    }
}
