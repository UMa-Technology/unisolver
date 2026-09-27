//! Renders a star field at a given FOV and pointing from a real Gaia catalog:
//!     cargo run --release -p unisolver-synth --example render_fov -- \
//!         <fov_deg> <ra_deg> <dec_deg> <out.png|out.jpg> [width height] [catalog.bin] [mag_limit]
//! PNG output is 16-bit; JPEG output is 8-bit with a percentile stretch. Used for
//! boundary checks (the 78–80° limit, the celestial poles) and synthetic samples.
use unisolver_synth::{look_at, render, RenderParams};

/// Reads a GDR3 binary (b"GDR3" + version u32 + count u64; 36 bytes per star:
/// source_id i64, ra f64, dec f64, mag f32, pmra f32, pmdec f32).
fn load_gdr3(path: &str) -> Vec<tetra3::Star> {
    let bytes = std::fs::read(path).expect("read catalog");
    assert_eq!(&bytes[0..4], b"GDR3", "not a GDR3 file");
    let count = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    (0..count)
        .map(|i| {
            let r = &bytes[16 + i * 36..16 + (i + 1) * 36];
            tetra3::Star {
                id: i64::from_le_bytes(r[0..8].try_into().unwrap()),
                ra_rad: (f64::from_le_bytes(r[8..16].try_into().unwrap()) as f32).to_radians(),
                dec_rad: (f64::from_le_bytes(r[16..24].try_into().unwrap()) as f32).to_radians(),
                mag: f32::from_le_bytes(r[24..28].try_into().unwrap()),
            }
        })
        .collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let fov: f32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(70.0);
    let ra: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let dec: f32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(40.0);
    let out = a.get(4).cloned().unwrap_or_else(|| "synth.png".into());
    let w: u32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(1920);
    let h: u32 = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(1080);
    let cat = a
        .get(7)
        .cloned()
        .unwrap_or_else(|| "third_party/tetra3/data/gaia_merged.bin".into());

    let stars = load_gdr3(&cat);
    eprintln!("{} stars from {cat}", stars.len());

    let q = look_at(ra as f64, dec as f64, 0.0);
    let mut params = RenderParams::default();
    if let Some(ml) = a.get(8).and_then(|s| s.parse::<f32>().ok()) {
        // Narrow fields need faint stars (the default 7.0 only suits wide fields). The
        // default calibration gives G8 200 photons, so a G13 star would drown at ~2;
        // scale with the limit so the faintest star keeps ~60 photons.
        params.mag_limit = ml;
        params.flux_mag8 = 60.0 * 10f32.powf(0.4 * (ml - 8.0));
    }
    let img = render(&stars, &q, fov, w, h, &params, 30);
    let lower = out.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        // 8-bit output needs a percentile stretch: dividing by the brightest
        // star quantises the faint ones to zero and the frame stops solving.
        let mut sorted = img.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p999 = sorted[(sorted.len() as f64 * 0.999) as usize].max(1.0);
        let buf: Vec<u8> = img
            .iter()
            .map(|v| ((v / p999).min(1.0) * 255.0) as u8)
            .collect();
        image::save_buffer(&out, &buf, w, h, image::ExtendedColorType::L8).expect("save");
    } else {
        let maxv = img.iter().cloned().fold(0.0f32, f32::max).max(1.0);
        let buf: Vec<u8> = img
            .iter()
            .flat_map(|v| (((v / maxv) * 65535.0) as u16).to_ne_bytes())
            .collect();
        image::save_buffer(&out, &buf, w, h, image::ExtendedColorType::L16).expect("save");
    }
    eprintln!("wrote {out} ({fov}deg at ra={ra} dec={dec}, {w}x{h})");
}
