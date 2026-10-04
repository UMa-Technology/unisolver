//! Desktop CLI: solve real photos against the production DB, emit JSON
//! (and optionally an annotated PNG) — the real-image test harness.
use clap::Parser;
use serde::Serialize;
use std::path::PathBuf;
use unisolver_core::*;

#[derive(Parser)]
struct Cli {
    /// Solver database (UNISOLV2 or upstream tetra3 format); required unless --pool is given
    #[arg(long)]
    db: Option<PathBuf>,
    #[arg(long)]
    dso: Option<PathBuf>,
    /// One or more images
    images: Vec<PathBuf>,
    /// Extraction profile: auto (adaptive, default) | phone (σ10) | clean (σ5)
    #[arg(long, default_value = "auto")]
    profile: String,
    /// Explicit σ threshold (selects the custom profile, overriding --profile)
    #[arg(long)]
    sigma: Option<f32>,
    #[arg(long, default_value_t = 100)]
    max_centroids: usize,
    /// Known horizontal FOV (skips the preset ladder)
    #[arg(long)]
    fov: Option<f32>,
    /// Write an annotated PNG per image into this directory
    #[arg(long)]
    annotate_dir: Option<PathBuf>,
    /// Write JSON results to this file
    #[arg(long)]
    out: Option<PathBuf>,
    /// Extraction statistics only (σ grid × blobs / centroids / brightness percentiles), no solve
    #[arg(long)]
    stats: bool,
    /// Burst tracking: blind-solve the first frame, then use the previous attitude as the hint (fast extraction)
    #[arg(long)]
    track: bool,
    /// After the staged ladder search fails, search every rung exhaustively (slow failures)
    #[arg(long)]
    thorough: bool,
    /// Keep the pinhole solve: do not fit the lens to wide frames' stars
    #[arg(long)]
    no_fit_lens: bool,
    /// Keep the scale the pattern match measured: do not re-measure it from the brightest stars
    #[arg(long)]
    no_refine_scale: bool,
    /// Calibration: feed every image to a CalibrationSession, fit radial distortion, write the camera JSON
    #[arg(long)]
    calibrate_out: Option<PathBuf>,
    /// Solve with a calibrated camera JSON (exclusive with --fov)
    #[arg(long)]
    camera: Option<PathBuf>,
    /// Multilingual names pack (UNAM) for annotation names; pick the language with `--language`
    #[arg(long)]
    names: Option<PathBuf>,
    /// Annotation language code (en / zh_cn / zh_tw / ja / ko / fr / de / es / it / ru / pl / hu / ro)
    #[arg(long, default_value = "en")]
    language: String,
    /// Observer `lat,lon[,alt_m]`: topocentric moon (up to 1° off without it); required by satellites
    #[arg(long)]
    observer: Option<String>,
    /// Satellite layer: a TLE text file (fetched by you; the engine never goes online). Needs --observer
    #[arg(long)]
    tle: Option<PathBuf>,
    /// Observation time (Unix ms), required by the solar-system and satellite layers. **Defaults to
    /// the header's time**: FITS/XISF DATE-AVG or DATE-OBS plus half the exposure, EXIF
    /// DateTimeOriginal when it carries its zone
    #[arg(long)]
    at_unix_ms: Option<i64>,
    /// Constellation pack (UCON): draws figures and IAU boundaries in the annotated PNGs
    #[arg(long)]
    constellations: Option<PathBuf>,
    /// Multi-tier routing: register every *.db in this directory, no database named (exclusive with
    /// --db). Ladder rungs are dispatched by tier range; the solving tier goes into the JSON `db` field.
    #[arg(long)]
    pool: Option<PathBuf>,
    /// Pointing hint for the narrow-field engine (--pool): right ascension, degrees. Needs
    /// --hint-dec. FITS/XISF files give one from the header when this is absent
    #[arg(long, requires = "hint_dec", allow_negative_numbers = true)]
    hint_ra: Option<f64>,
    /// Pointing hint: declination, degrees
    #[arg(long, requires = "hint_ra", allow_negative_numbers = true)]
    hint_dec: Option<f64>,
    /// Pointing hint search radius, degrees (default: one FOV)
    #[arg(long, requires = "hint_ra")]
    hint_radius: Option<f64>,
    /// --pool with an unknown FOV: after every tetra3 tier failed, blind-solve once with the
    /// narrow-field engine
    #[arg(long)]
    narrow_blind: bool,
}

#[derive(Serialize)]
struct ImageResult {
    file: String,
    width: u32,
    height: u32,
    status: String,
    /// Tier that solved it (--pool only)
    #[serde(skip_serializing_if = "Option::is_none")]
    db: Option<String>,
    attempts: Vec<AttemptOut>,
    num_centroids: usize,
    /// Median centroid axis ratio (trailing diagnostic; > 2 is clearly trailed)
    median_elongation: Option<f32>,
    solution: Option<SolutionOut>,
    extract_ms: f32,
    total_ms: f32,
}

#[derive(Serialize)]
struct AttemptOut {
    fov_deg: f32,
    status: String,
    solve_ms: f32,
    /// Tier used for this attempt (--pool only)
    #[serde(skip_serializing_if = "Option::is_none")]
    db: Option<String>,
    /// Engine of this attempt (--pool only)
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<TierKind>,
}

#[derive(Serialize)]
struct SolutionOut {
    ra_deg: f64,
    dec_deg: f64,
    roll_deg: f64,
    fov_deg: f32,
    num_matches: u32,
    rmse_arcsec: f32,
    p90_arcsec: f32,
    prob: f64,
    scale_arcsec_per_px: f64,
    /// The lens was fitted to this frame's stars
    lens_fitted: bool,
    /// The scale was re-measured from the brightest stars
    scale_refined: bool,
    named_stars: Vec<String>,
    dso: Vec<String>,
    /// Solar-system bodies (with --at-unix-ms; the moon is topocentric only with --observer)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    solar: Vec<String>,
    /// Satellites (--tle + --observer + --at-unix-ms)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    satellites: Vec<String>,
    /// Why a layer is unavailable or degraded, as `layer: message`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    layer_notes: Vec<String>,
}

fn status_str(s: SolveStatus) -> String {
    format!("{s:?}")
}

/// --sigma given → Custom; otherwise the profile from --profile
fn cli_profile(cli: &Cli) -> ExtractionProfile {
    if let Some(sig) = cli.sigma {
        return ExtractionProfile::Custom(ExtractionOptions::Ccl {
            sigma_threshold: sig,
            max_centroids: cli.max_centroids,
        });
    }
    match cli.profile.as_str() {
        "clean" => ExtractionProfile::CleanSensor,
        "phone" => ExtractionProfile::PhoneJpeg,
        _ => ExtractionProfile::Auto,
    }
}

fn draw_circle(img: &mut image::RgbImage, cx: f64, cy: f64, r: i32, color: [u8; 3]) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let steps = (r.max(4) * 8) as usize;
    for k in 0..steps {
        let a = k as f64 / steps as f64 * std::f64::consts::TAU;
        let (x, y) = (
            (cx + r as f64 * a.cos()) as i32,
            (cy + r as f64 * a.sin()) as i32,
        );
        if x >= 0 && y >= 0 && x < w && y < h {
            img.put_pixel(x as u32, y as u32, image::Rgb(color));
        }
    }
}

fn draw_line(img: &mut image::RgbImage, a: [f64; 2], b: [f64; 2], color: [u8; 3]) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let steps = ((b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil() as usize).max(1);
    for k in 0..=steps {
        let t = k as f64 / steps as f64;
        let (x, y) = (
            (a[0] + (b[0] - a[0]) * t) as i32,
            (a[1] + (b[1] - a[1]) * t) as i32,
        );
        if x >= 0 && y >= 0 && x < w && y < h {
            img.put_pixel(x as u32, y as u32, image::Rgb(color));
        }
    }
}

/// Loads any supported format (imageio: FITS/XISF/PNG/JPEG/TIFF, by magic bytes)
fn load_frame(
    path: &std::path::Path,
) -> anyhow_like::Result<(Frame, unisolver_core::imageio::ImageMeta)> {
    Ok(unisolver_core::imageio::load_image(path.to_str().unwrap())?)
}
mod anyhow_like {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}

/// `lat,lon[,alt_m]` → Observer (core's validate checks ranges; this only checks the shape)
fn parse_observer(s: &str) -> std::result::Result<Observer, String> {
    let p: Vec<f64> = s
        .split(',')
        .map(|t| t.trim().parse::<f64>().map_err(|e| e.to_string()))
        .collect::<std::result::Result<_, _>>()?;
    match p.len() {
        2 => Ok(Observer {
            lat_deg: p[0],
            lon_deg: p[1],
            alt_m: 0.0,
        }),
        3 => Ok(Observer {
            lat_deg: p[0],
            lon_deg: p[1],
            alt_m: p[2],
        }),
        n => Err(format!("--observer wants lat,lon[,alt_m]; got {n} numbers")),
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let observer = match &cli.observer {
        Some(s) => {
            let o = parse_observer(s)?;
            o.validate()?;
            Some(o)
        }
        None => None,
    };
    let tle_text = match &cli.tle {
        Some(p) => Some(std::fs::read_to_string(p)?),
        None => None,
    };
    if tle_text.is_some() && observer.is_none() {
        return Err("--tle needs --observer too (satellite positions are topocentric)".into());
    }
    if cli.pool.is_some() && (cli.track || cli.calibrate_out.is_some()) {
        return Err(
            "--pool does not support --track / --calibrate-out (both bind a single database)"
                .into(),
        );
    }
    let pool = match &cli.pool {
        Some(dir) => {
            let (p, skipped) = SolverPool::open_dir(dir.to_str().unwrap())?;
            for (f, e) in &skipped {
                eprintln!("pool: skipped {f}: {e}");
            }
            for t in p.tiers() {
                eprintln!(
                    "pool: {} [{:.2}–{:.2}°] {} stars / {} patterns{}",
                    t.name,
                    t.min_fov_deg,
                    t.max_fov_deg,
                    t.num_stars,
                    t.num_patterns,
                    if t.kind == TierKind::Narrow {
                        " (narrow-field engine)"
                    } else {
                        ""
                    }
                );
            }

            Some(p)
        }
        None => None,
    };
    // With --pool no single database is opened
    let solver = match &pool {
        Some(_) => None,
        None => {
            let db = cli.db.as_ref().ok_or("give --db or --pool")?;
            Some(Solver::from_file(db.to_str().unwrap())?)
        }
    };
    let solver_of = |name: Option<&str>| -> std::result::Result<&Solver, String> {
        match &pool {
            Some(p) => p
                .annotation_solver(name)
                .ok_or_else(|| "the pool has no tetra3 tier to annotate with".to_string()),
            None => Ok(solver.as_ref().expect("single-db mode")),
        }
    };

    // ── --stats: σ-grid extraction statistics, no solve ──
    if cli.stats {
        for path in &cli.images {
            let (frame, _meta) = load_frame(path)?;
            let luma = frame.to_luma_f32()?;
            for sigma in [3.5f32, 5.0, 7.0, 10.0, 14.0] {
                let ext = tetra3::extract_centroids_from_raw(
                    &luma,
                    frame.width,
                    frame.height,
                    &tetra3::CentroidExtractionConfig {
                        sigma_threshold: sigma,
                        max_centroids: Some(cli.max_centroids),
                        ..Default::default()
                    },
                )?;
                let mut masses: Vec<f32> = ext.centroids.iter().filter_map(|c| c.mass).collect();
                masses.sort_by(f32::total_cmp);
                let pick = |q: f64| {
                    masses
                        .get(((masses.len() as f64 - 1.0) * q) as usize)
                        .copied()
                        .unwrap_or(0.0)
                };
                println!(
                    "STATS {} sigma={sigma} blobs_raw={} centroids={} mass_p50={:.1} mass_p90={:.1}",
                    path.display(),
                    ext.num_blobs_raw,
                    ext.centroids.len(),
                    pick(0.5),
                    pick(0.9)
                );
            }
        }
        return Ok(());
    }

    // ── Calibration: a separate flow ──
    if let Some(calib_out) = &cli.calibrate_out {
        let mut session = solver_of(None)?.new_calibration_session()?;
        for path in &cli.images {
            let dyn_img = image::open(path)?;
            let gray = dyn_img.to_luma32f();
            let (w, h) = (gray.width(), gray.height());
            let frame = Frame {
                width: w,
                height: h,
                row_stride_bytes: None,
                pixels: PixelData::LumaF32(gray.into_raw()),
            };
            let mut o = SolveOptions::new(cli.fov.unwrap_or(70.0));
            o.fov_max_error_deg = Some(8.0);
            o.extraction = cli_profile(&cli);
            o.timeout_ms = Some(6_000);
            let out = session.add_frame(&frame, &o)?;
            println!(
                "calib frame {}: {:?} ({} in session)",
                path.display(),
                out.status,
                session.count()
            );
        }
        let rep = session.fit(CalibModel::Radial)?;
        println!(
            "fit: rmse {:.2}px -> {:.2}px, inliers {}, outliers {}, frames {}",
            rep.rmse_before_px, rep.rmse_after_px, rep.n_inliers, rep.n_outliers, rep.frames_used
        );
        std::fs::write(calib_out, serde_json::to_string_pretty(&rep.camera)?)?;
        println!("camera params -> {}", calib_out.display());
        return Ok(());
    }

    let camera_params: Option<CameraParams> = match &cli.camera {
        Some(p) => Some(serde_json::from_str(&std::fs::read_to_string(p)?)?),
        None => None,
    };
    // Annotate with the catalog of **the tier that solved it** (narrow tiers are denser); one annotator per tier
    let dso_arg = cli.dso.as_ref().and_then(|p| p.to_str());
    let names_arg = cli.names.as_ref().and_then(|p| p.to_str());
    let mut annotators: std::collections::HashMap<String, Annotator> =
        std::collections::HashMap::new();
    let mut results = Vec::new();
    let mut prev: Option<([f32; 4], f32)> = None;

    for path in &cli.images {
        let t0 = std::time::Instant::now();
        let (frame, meta) = load_frame(path)?;
        let (w, h) = (frame.width, frame.height);
        let is_astro = matches!(
            meta.source,
            unisolver_core::imageio::SourceFormat::Fits
                | unisolver_core::imageio::SourceFormat::Xisf
        );
        let mut base = SolveOptions::new(cli.fov.unwrap_or(70.0));
        base.extraction = if cli.sigma.is_none() && cli.profile == "auto" {
            let _ = is_astro;
            unisolver_core::imageio::suggested_profile(&meta)
        } else {
            cli_profile(&cli)
        };
        base.timeout_ms = Some(4_000);
        base.thorough = cli.thorough;
        base.fit_lens = !cli.no_fit_lens;
        base.refine_scale = !cli.no_refine_scale;

        // Observation time and place: the command line first, then the header (FITS/XISF time,
        // EXIF time with its zone and GPS position), as the library's file entries do
        base.observation_unix_ms = cli.at_unix_ms;
        meta.apply_time(&mut base);
        let at_unix_ms = base.observation_unix_ms;
        let observer = observer.or(meta.observer);
        let track_hint = if cli.track { prev } else { None };
        let mut db_used: Option<String> = None;
        let (out, attempts) = if let Some(p) = &pool {
            // Pool routing: with --fov only that rung is tried (the pool still picks tiers covering it);
            // otherwise header hints + aspect ladder, plus range sweeps for tiers never reached
            let hints = match cli.fov {
                Some(fov) => vec![FovPreset {
                    fov_deg: fov,
                    max_error_deg: (fov * 0.12).max(0.3),
                }],
                None => unisolver_core::presets_with_hints(&meta, w, h),
            };
            // Calibrated camera: the focal length fixes the FOV; the pool picks tiers by it, no ladder
            let mut base = base.clone();
            if let Some(cam) = &camera_params {
                base.fov_estimate_deg = cam.horizontal_fov_deg(w) as f32;
                base.camera = Some(cam.clone());
                base.fov_max_error_deg = None;
            }
            base.pointing_hint =
                cli.hint_ra
                    .zip(cli.hint_dec)
                    .map(|(ra_deg, dec_deg)| PointingHint {
                        ra_deg,
                        dec_deg,
                        radius_deg: cli.hint_radius,
                    });
            base.narrow_blind = cli.narrow_blind;
            meta.apply_pointing(&mut base);
            let r = p.solve_auto(&frame, &base, &hints)?;
            db_used = r.db;
            let a = r
                .attempts
                .iter()
                .map(|x| AttemptOut {
                    fov_deg: x.fov_deg,
                    status: status_str(x.status),
                    solve_ms: x.solve_ms,
                    db: Some(x.db.clone()),
                    kind: Some(x.kind),
                })
                .collect();
            (r.outcome, a)
        } else {
            let single = solver.as_ref().expect("single-db mode");
            let (out, attempts) = if let Some((hint, fov)) = track_hint {
                let mut o = base.clone();
                o.fov_estimate_deg = fov;
                o.camera = None;
                o.attitude_hint = Some(hint);
                o.hint_uncertainty_deg = 3.0;
                o.extraction = ExtractionProfile::Custom(ExtractionOptions::Fast {
                    sigma_threshold: cli.sigma.unwrap_or(10.0),
                    max_centroids: 60,
                });
                let mut out = single.solve(&frame, &o)?;
                let mut a = vec![FovAttempt {
                    fov_deg: fov,
                    status: out.status,
                    solve_ms: out.timing.solve_ms,
                }];
                if !matches!(out.status, SolveStatus::Ok) {
                    // Fast extraction is weak on dark frames: retry with CCL + hint (still fast), else count it as failed
                    o.extraction = cli_profile(&cli);
                    out = single.solve(&frame, &o)?;
                    a.push(FovAttempt {
                        fov_deg: fov,
                        status: out.status,
                        solve_ms: out.timing.solve_ms,
                    });
                }
                (out, a)
            } else if let Some(cam) = &camera_params {
                let mut o = base.clone();
                o.camera = Some(cam.clone());
                o.fov_estimate_deg = cam.horizontal_fov_deg(w) as f32;
                o.fov_max_error_deg = None; // calibrated: the focal length fixes the FOV, no sweep
                let out = single.solve(&frame, &o)?;
                let a = vec![FovAttempt {
                    fov_deg: o.fov_estimate_deg,
                    status: out.status,
                    solve_ms: out.timing.solve_ms,
                }];
                (out, a)
            } else if let Some(fov) = cli.fov {
                let mut o = base.clone();
                o.fov_estimate_deg = fov;
                o.fov_max_error_deg = Some(fov * 0.12);
                let out = single.solve(&frame, &o)?;
                let a = vec![FovAttempt {
                    fov_deg: fov,
                    status: out.status,
                    solve_ms: out.timing.solve_ms,
                }];
                (out, a)
            } else {
                // Header hints (untrusted: only placed first, ±15%) + aspect ladder (single source in core)
                let presets = unisolver_core::presets_with_hints(&meta, w, h);
                single.solve_with_fov_presets(&frame, &base, &presets)?
            };
            let a = attempts
                .iter()
                .map(|x| AttemptOut {
                    fov_deg: x.fov_deg,
                    status: status_str(x.status),
                    solve_ms: x.solve_ms,
                    db: None,
                    kind: None,
                })
                .collect();
            (out, a)
        };

        let ann_key = db_used.clone().unwrap_or_default();
        if !annotators.contains_key(&ann_key) {
            annotators.insert(
                ann_key.clone(),
                solver_of(db_used.as_deref())?
                    .annotator(dso_arg, names_arg)?
                    .with_constellations(cli.constellations.as_ref().and_then(|p| p.to_str())),
            );
        }
        let annotator = &annotators[&ann_key];

        if let Some(g) = out.solution.as_ref() {
            prev = Some((g.quat_icrs2cam_wxyz, g.fov_deg));
        }
        let solution = out.solution.as_ref().map(|g| {
            let ann = annotator.annotate(
                &g.wcs,
                &AnnotateOptions {
                    language: cli.language.clone(),
                    observation_unix_ms: at_unix_ms,
                    observer,
                    satellite_tle: tle_text.clone(),
                    ..Default::default()
                },
            );
            SolutionOut {
                ra_deg: g.ra_deg,
                dec_deg: g.dec_deg,
                roll_deg: g.roll_deg,
                fov_deg: g.fov_deg,
                num_matches: g.num_matches,
                rmse_arcsec: g.rmse_arcsec,
                p90_arcsec: g.p90_arcsec,
                prob: g.prob,
                scale_arcsec_per_px: g.wcs.scale_arcsec_per_px(),
                lens_fitted: g.lens_fitted,
                scale_refined: g.scale_refined,
                named_stars: ann
                    .named_stars
                    .iter()
                    .map(|n| format!("{} ({:.0},{:.0})", n.name, n.x, n.y))
                    .collect(),
                dso: ann
                    .objects
                    .iter()
                    .take(12)
                    .map(|o| {
                        format!(
                            "{}{} ({:.0},{:.0}) r={:.0}px{}",
                            o.designation,
                            o.common_name
                                .as_deref()
                                .map(|c| format!(" {c}"))
                                .unwrap_or_default(),
                            o.x,
                            o.y,
                            o.semi_major_px,
                            if o.outlines.is_empty() {
                                String::new()
                            } else {
                                format!(" outline levels={}", o.outlines.len())
                            }
                        )
                    })
                    .collect(),
                solar: ann
                    .solar
                    .iter()
                    .map(|b| format!("{} ({:.0},{:.0})", b.name, b.x, b.y))
                    .collect(),
                satellites: ann
                    .satellites
                    .iter()
                    .map(|s| format!("{} ({:.0},{:.0}) {:.0}km", s.name, s.x, s.y, s.range_km))
                    .collect(),
                layer_notes: ann
                    .layers
                    .reasons
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect(),
            }
        });

        if let (Some(dir), Some(g)) = (cli.annotate_dir.as_ref(), out.solution.as_ref()) {
            std::fs::create_dir_all(dir)?;
            // Annotation background: rasters open directly; astro formats get the engine's
            // auto-stretched preview at full size
            let mut rgb = match image::open(path) {
                Ok(d) => d.to_rgb8(),
                Err(_) => {
                    let pv = unisolver_core::imageio::preview(&frame, w.max(h))?;
                    image::RgbImage::from_fn(w, h, |x, y| {
                        let g = pv.luma[(y * w + x) as usize];
                        image::Rgb([g, g, g])
                    })
                }
            };
            for c in &out.centroids {
                draw_circle(&mut rgb, c.x, c.y, 9, [64, 255, 64]);
            }
            for m in &g.matched {
                draw_circle(&mut rgb, m.x, m.y, 12, [255, 80, 80]);
            }
            let ann = annotator.annotate(
                &g.wcs,
                &AnnotateOptions {
                    language: cli.language.clone(),
                    // Keep the overlay clean: bright stars and well-known deep-sky objects (dso_max_mag drops IC
                    // entries without a magnitude)
                    star_max_mag: Some(5.0),
                    max_stars: 120,
                    dso_max_mag: Some(8.0),
                    observation_unix_ms: at_unix_ms,
                    observer,
                    satellite_tle: tle_text.clone(),
                    include_constellations: cli.constellations.is_some(),
                    constellation_boundaries: cli.constellations.is_some(),
                    ..Default::default()
                },
            );
            for b in &ann.boundaries {
                for s in b.points.windows(2) {
                    draw_line(&mut rgb, s[0], s[1], [110, 110, 130]);
                }
            }
            for c in &ann.constellations {
                for s in c.lines.iter().flat_map(|l| l.windows(2)) {
                    draw_line(&mut rgb, s[0], s[1], [120, 170, 255]);
                }
            }
            for s in &ann.stars {
                draw_circle(&mut rgb, s.x, s.y, 5, [255, 210, 60]);
            }
            for n in &ann.named_stars {
                draw_circle(&mut rgb, n.x, n.y, 16, [80, 200, 255]);
            }
            for o in &ann.objects {
                if o.outlines.is_empty() {
                    draw_circle(
                        &mut rgb,
                        o.x,
                        o.y,
                        o.semi_major_px.max(14.0) as i32,
                        [200, 120, 255],
                    );
                }
                for c in o.outlines.iter().flat_map(|l| &l.contours) {
                    for s in c.points.windows(2) {
                        draw_line(&mut rgb, s[0], s[1], [200, 120, 255]);
                    }
                    if let (true, Some(a), Some(b)) = (c.closed, c.points.last(), c.points.first())
                    {
                        draw_line(&mut rgb, *a, *b, [200, 120, 255]);
                    }
                }
            }
            for b in &ann.solar {
                draw_circle(
                    &mut rgb,
                    b.x,
                    b.y,
                    b.angular_radius_px.unwrap_or(10.0).max(10.0) as i32,
                    [255, 160, 60],
                );
            }
            for s in &ann.satellites {
                draw_circle(&mut rgb, s.x, s.y, 7, [120, 255, 255]);
            }
            let name = path.file_stem().unwrap().to_string_lossy();
            rgb.save(dir.join(format!("{name}_annotated.png")))?;
        }

        let r = ImageResult {
            file: path.to_string_lossy().into_owned(),
            width: w,
            height: h,
            status: out
                .solution
                .as_ref()
                .map(|_| "Ok".to_string())
                .unwrap_or_else(|| status_str(out.status)),
            db: db_used.clone(),
            attempts,
            num_centroids: out.centroids.len(),
            median_elongation: out.median_elongation,
            solution,
            extract_ms: out.timing.extract_ms,
            total_ms: t0.elapsed().as_secs_f32() * 1000.0,
        };
        println!(
            "{}: {} ({} centroids{}, {:.0} ms{})",
            r.file,
            r.status,
            r.num_centroids,
            // Star-shape diagnostic: the first thing to check on a failure, so it is printed then too.
            // No "trailed / normal" verdict: the threshold depends on pixel scale and PSF (see core docs)
            match r.median_elongation {
                Some(e) => format!(", elong {e:.2}"),
                None => String::new(),
            },
            r.total_ms,
            r.solution
                .as_ref()
                .map(|s| format!(
                    ", ra={:.3} dec={:.3} fov={:.1} matches={} rmse={:.0}\"{}{}",
                    s.ra_deg,
                    s.dec_deg,
                    s.fov_deg,
                    s.num_matches,
                    s.rmse_arcsec,
                    if s.scale_refined { " scale" } else { "" },
                    if s.lens_fitted { " lens" } else { "" }
                ))
                .unwrap_or_default()
        );
        results.push(r);
    }

    if let Some(out) = cli.out {
        std::fs::write(out, serde_json::to_string_pretty(&results)?)?;
    }
    let ok = results.iter().filter(|r| r.status == "Ok").count();
    eprintln!("solved {ok}/{} images", results.len());
    Ok(())
}
