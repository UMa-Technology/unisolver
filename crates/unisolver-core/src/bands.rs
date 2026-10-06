//! Banded extraction for large frames. The connected-component extraction allocates
//! several full-frame f32 buffers (residuals, the matched filter's output) on top of the
//! caller's luminance copy: about 580 MB peak on a 26 Mpx frame. Extracting horizontal
//! bands instead bounds that by the band, and the bands are converted from the frame's own
//! pixels, so no full-frame f32 copy exists at all.
//!
//! A band is the same computation on fewer rows, provided every star is measured with its
//! surroundings intact:
//! - band edges fall on multiples of the 64 px background block, so a band's block grid is
//!   the frame's (medians of the same pixels) and its bilinear background is identical away
//!   from the band edges;
//! - each band carries a 128 px margin above and below (two blocks: the bilinear reach is one
//!   block, the matched filter a few pixels, the per-star background annulus and the largest
//!   blob well under that) and keeps only stars centred in its core;
//! - a star on a core boundary is seen by both bands; the one measuring it deeper inside
//!   its core wins.
//!
//! One extractor and one luminance buffer serve every band, largest band first, so the
//! working memory is allocated once: fresh buffers per band left the allocator holding the
//! previous band's and nearly doubled the peak.
//!
//! What still differs is the noise estimate (per band instead of per frame), which moves the
//! detection threshold slightly: the faintest detections can change, the brightest ones that
//! make up the solve do not (see the tests and the local check on real 26 Mpx frames).
use crate::{Frame, Result};
use tetra3::centroid_extraction::{CentroidExtractionConfig, CentroidExtractor};
use tetra3::Centroid;

/// Frames above this size are extracted in bands (26 Mpx astro frames, 48 Mpx phone
/// originals); phone frames up to 4K keep the single pass and its exact results.
pub(crate) const BANDED_ABOVE_PX: usize = 16_000_000;
/// Pixels in a band's core: 640 rows on a 6248 px wide frame, about 100 MB of working
/// buffers per band.
const BAND_CORE_PX: usize = 4_000_000;
/// tetra3's local background block (the `CentroidExtractionConfig` default this relies on)
const BLOCK: usize = 64;
const MARGIN: usize = 2 * BLOCK;
/// Stars this close to a core boundary may be seen by both bands (in pixels)
const EDGE: f64 = 2.0;
/// Two detections closer than this in both axes are the same star
const SAME: f32 = 1.5;

/// Band cores `(y0, y1)` for a frame of `w × h`: multiples of the background block.
fn cores(w: usize, h: usize) -> Vec<(usize, usize)> {
    let rows = (BAND_CORE_PX / w.max(1)).max(BLOCK).next_multiple_of(BLOCK);
    (0..h)
        .step_by(rows)
        .map(|y0| (y0, (y0 + rows).min(h)))
        .collect()
}

/// A detection in frame coordinates: how deep inside its band's core it lies, its band, and its
/// rank in that band's extraction (brightest first)
#[derive(Clone)]
struct Detection {
    centroid: Centroid,
    depth: f64,
    band: usize,
    rank: usize,
}

/// Connected-component extraction of `frame` band by band, as two lists that each match a
/// single pass (centre-origin coordinates, brightest first). The first holds at most
/// `cfg.max_centroids`, each band capped alike before the bands merge, as earlier releases
/// extracted it, so the solves that take it do not change; the second holds every detection,
/// for the wide-field spread (`crate::spread`).
pub(crate) fn extract(
    frame: &Frame,
    cfg: &CentroidExtractionConfig,
) -> Result<(Vec<Centroid>, Vec<Centroid>)> {
    assert_eq!(
        cfg.local_bg_block_size,
        Some(BLOCK as u32),
        "bands align to the background block"
    );
    let uncapped = CentroidExtractionConfig {
        max_centroids: None,
        ..cfg.clone()
    };
    let found = detect(frame, &uncapped)?;
    let cap = cfg.max_centroids.unwrap_or(usize::MAX);
    let mut capped = merge(found.iter().filter(|d| d.rank < cap).cloned().collect());
    capped.truncate(cap);
    Ok((capped, merge(found)))
}

/// Every band's detections centred in its core (or within `EDGE` of it)
fn detect(frame: &Frame, cfg: &CentroidExtractionConfig) -> Result<Vec<Detection>> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let half_h = (h as f64 - 1.0) / 2.0;
    // Bands with their margins, largest first (the buffers are sized once)
    let mut bands: Vec<(usize, usize, usize, usize)> = cores(w, h)
        .into_iter()
        .map(|(y0, y1)| (y0, y1, y0.saturating_sub(MARGIN), (y1 + MARGIN).min(h)))
        .collect();
    bands.sort_by_key(|&(y0, _, top, bottom)| (std::cmp::Reverse(bottom - top), y0));
    let mut extractor = CentroidExtractor::new();
    let mut luma = Vec::new();
    let mut found = Vec::new();
    for (y0, y1, top, bottom) in bands {
        frame.to_luma_f32_rows_into(top, bottom, &mut luma)?;
        let r = extractor.extract_from_raw(&luma, w as u32, (bottom - top) as u32, cfg)?;
        // Band centre-origin → frame centre-origin
        let shift = top as f64 + (bottom - top - 1) as f64 / 2.0 - half_h;
        for (rank, mut c) in r.centroids.into_iter().enumerate() {
            let y = c.y as f64 + shift;
            let row = y + half_h;
            let depth = (row - y0 as f64).min(y1 as f64 - row);
            if depth >= -EDGE {
                c.y = y as f32;
                found.push(Detection {
                    centroid: c,
                    depth,
                    band: y0,
                    rank,
                });
            }
        }
    }
    Ok(found)
}

/// One list from the bands' detections: a star on a core boundary, seen by both bands, keeps
/// the measurement from the band that saw it deepest inside its core; brightest first, as a
/// single pass returns them (ties in raster order).
fn merge(mut found: Vec<Detection>) -> Vec<Centroid> {
    found.sort_by(|a, b| b.depth.total_cmp(&a.depth));
    let mut kept: Vec<Centroid> = Vec::with_capacity(found.len());
    let mut boundary: Vec<(f32, f32, usize)> = Vec::new();
    for d in found {
        let c = d.centroid;
        if d.depth < EDGE {
            let seen = boundary
                .iter()
                .any(|&(x, y, b)| b != d.band && (x - c.x).abs() < SAME && (y - c.y).abs() < SAME);
            if seen {
                continue;
            }
            boundary.push((c.x, c.y, d.band));
        }
        kept.push(c);
    }
    kept.sort_by(|a, b| {
        b.mass
            .unwrap_or(0.0)
            .total_cmp(&a.mass.unwrap_or(0.0))
            .then(a.y.total_cmp(&b.y))
            .then(a.x.total_cmp(&b.x))
    });
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PixelData;

    #[test]
    fn cores_tile_the_frame_on_block_boundaries() {
        let c = cores(6248, 4176);
        assert_eq!(c.first(), Some(&(0, 640)));
        assert_eq!(c.last().unwrap().1, 4176);
        assert!(c.windows(2).all(|p| p[0].1 == p[1].0));
        assert!(c.iter().all(|&(y0, _)| y0 % BLOCK == 0));
        // A narrow frame still gets whole blocks per band
        assert_eq!(
            cores(100_000, 300),
            vec![(0, 64), (64, 128), (128, 192), (192, 256), (256, 300)]
        );
    }

    /// The private 26 Mpx frames (6248 × 4176): the banded top 100 against the single pass.
    /// Per-band noise estimates move the threshold a little, so a few near-equal faint stars
    /// can trade places (19 of 100 on M24's dense star cloud) and a blended pair can split
    /// differently; the positions of the stars both keep agree to hundredths of a pixel, and
    /// the solves are unchanged (recorded in the maintainers' report). Skipped without the
    /// corpus.
    #[cfg(feature = "imageio")]
    #[test]
    fn real_large_frames_match_the_single_pass() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private/local");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            eprintln!("skipped: needs the private local frames");
            return;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "fits"))
            .collect();
        files.sort();
        let cfg = CentroidExtractionConfig {
            sigma_threshold: 5.0,
            max_centroids: Some(100),
            ..Default::default()
        };
        for f in files {
            let (frame, _) = crate::imageio::load_image(f.to_str().unwrap()).unwrap();
            if (frame.width as usize) * (frame.height as usize) <= BANDED_ABOVE_PX {
                continue;
            }
            let luma = frame.to_luma_f32().unwrap();
            let single = tetra3::centroid_extraction::extract_centroids_from_raw(
                &luma,
                frame.width,
                frame.height,
                &cfg,
            )
            .unwrap()
            .centroids;
            drop(luma);
            let banded = extract(&frame, &cfg).unwrap().0;
            let mut diffs: Vec<f32> = single
                .iter()
                .filter_map(|s| {
                    banded
                        .iter()
                        .find(|b| (b.x - s.x).abs() < 0.5 && (b.y - s.y).abs() < 0.5)
                        .map(|b| (b.x - s.x).abs().max((b.y - s.y).abs()))
                })
                .collect();
            diffs.sort_by(f32::total_cmp);
            let median = diffs[diffs.len() / 2];
            eprintln!(
                "{}: {}/{} of the single pass's stars kept, position difference median {median:.4} px, max {:.3} px",
                f.file_name().unwrap().to_string_lossy(),
                diffs.len(),
                single.len(),
                diffs.last().unwrap()
            );
            assert!(diffs.len() >= 80, "{}", diffs.len());
            assert!(median < 0.02, "{median}");
        }
    }

    /// A synthetic field large enough to band (4096 × 4096 with stars across every band
    /// boundary): the banded result equals the single pass star for star.
    #[test]
    fn bands_reproduce_the_single_pass() {
        let (w, h) = (4096u32, 4096u32);
        let q = unisolver_synth::look_at(80.0, 30.0, 10.0);
        let stars = unisolver_synth::random_sky(200_000, 7, 0.5, 11.0);
        let params = unisolver_synth::RenderParams {
            mag_limit: 10.0,
            ..Default::default()
        };
        let img = unisolver_synth::render(&stars, &q, 12.0, w, h, &params, 3);
        let frame = Frame {
            width: w,
            height: h,
            row_stride_bytes: None,
            pixels: PixelData::LumaF32(img.clone()),
        };
        let cfg = CentroidExtractionConfig {
            sigma_threshold: 5.0,
            max_centroids: Some(100),
            ..Default::default()
        };
        let single = tetra3::centroid_extraction::extract_centroids_from_raw(&img, w, h, &cfg)
            .unwrap()
            .centroids;
        let (banded, all) = extract(&frame, &cfg).unwrap();
        assert_eq!(single.len(), 100);
        assert_eq!(banded.len(), single.len());
        let near_boundary = banded
            .iter()
            .filter(|c| {
                let row = c.y as f64 + (h as f64 - 1.0) / 2.0;
                cores(w as usize, h as usize)
                    .iter()
                    .any(|&(y0, _)| y0 > 0 && (row - y0 as f64).abs() < 20.0)
            })
            .count();
        assert!(
            near_boundary > 0,
            "the field must put stars near band boundaries"
        );
        for s in &single {
            let b = banded
                .iter()
                .find(|b| (b.x - s.x).abs() < 0.05 && (b.y - s.y).abs() < 0.05)
                .unwrap_or_else(|| panic!("star at ({}, {}) missing from the bands", s.x, s.y));
            let dm = (b.mass.unwrap() - s.mass.unwrap()).abs() / s.mass.unwrap();
            assert!(dm < 0.01, "mass differs by {dm}");
        }
        // Every detection comes back too, brightest first, and holds each star of the capped list
        assert!(all.len() > banded.len(), "{} detections", all.len());
        assert!(all.windows(2).all(|p| p[0].mass >= p[1].mass));
        for b in &banded {
            assert!(
                all.iter()
                    .any(|a| (a.x - b.x).abs() < 0.05 && (a.y - b.y).abs() < 0.05),
                "capped star at ({}, {}) missing from every detection",
                b.x,
                b.y
            );
        }
    }
}
