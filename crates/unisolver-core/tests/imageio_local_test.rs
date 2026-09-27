//! Real-file checks on the private testdata/private/local/*.fits; prints skipped when absent.
use unisolver_core::imageio::load_image;

#[test]
fn local_fits_parse_and_hints() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let m24 = root.join("testdata/private/local/M24.fits");
    if !m24.exists() {
        eprintln!("skipped: local FITS not present");
        return;
    }
    let (frame, meta) = load_image(m24.to_str().unwrap()).unwrap();
    assert_eq!((frame.width, frame.height), (6248, 4176));
    let hints = meta.solve_hints();
    assert!(!hints.is_empty());
    assert!(
        (hints[0].fov_deg - 7.65).abs() < 0.1,
        "fov={}",
        hints[0].fov_deg
    );
    // m42.fits: DSLR simulator without focal-length keywords: loads and gives no hint
    let m42 = root.join("testdata/private/local/m42.fits");
    if m42.exists() {
        let (_, meta) = load_image(m42.to_str().unwrap()).unwrap();
        assert!(meta.solve_hints().is_empty());
    }
    // Cooled camera frame: 16-bit BZERO, 600 s, Bayer keywords
    let asi = root.join(
        "testdata/private/local/2026-03-19_00-03-09__-9.90_600.00s_00132026-03-102026-03-10.fits",
    );
    if asi.exists() {
        let (_, m) = load_image(asi.to_str().unwrap()).unwrap();
        assert_eq!(m.exposure_s, Some(600.0));
        let h = m.solve_hints();
        assert!(
            (h[0].fov_deg - 2.95).abs() < 0.05,
            "asi fov={}",
            h[0].fov_deg
        );
    }
}
