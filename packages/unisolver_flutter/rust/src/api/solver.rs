//! Handle API: UniSolver / UniAnnotator / UniCalibration on the Dart side.
//! Every error is an anyhow error (FRB → Dart AnyhowException carrying the message).
use crate::api::types::*;
use anyhow::Result;
use flutter_rust_bridge::frb;
use unisolver_core as core;

#[frb(opaque)]
pub struct UniSolver {
    inner: core::Solver,
}

impl UniSolver {
    pub fn new(db_path: String) -> Result<UniSolver> {
        Ok(Self {
            inner: core::Solver::from_file(&db_path)?,
        })
    }

    #[frb(sync)]
    pub fn properties(&self) -> DbPropertiesDto {
        let p = self.inner.properties();
        DbPropertiesDto {
            min_fov_deg: p.min_fov_deg,
            max_fov_deg: p.max_fov_deg,
            num_stars: p.num_stars as u64,
            num_patterns: p.num_patterns,
            star_max_magnitude: p.star_max_magnitude,
        }
    }

    pub fn solve_frame(&self, frame: FrameDto, opts: SolveOptionsDto) -> Result<SolveOutcomeDto> {
        let f: core::Frame = frame.try_into()?;
        let o: core::SolveOptions = opts.try_into()?;
        Ok(self.inner.solve(&f, &o)?.into())
    }

    pub fn solve_image_file(&self, path: String, opts: SolveOptionsDto) -> Result<SolveOutcomeDto> {
        let (f, meta, o) = load_with_header_time(&path, opts)?;
        let mut out = self.inner.solve(&f, &o)?;
        meta.apply_place(&mut out);
        Ok(out.into())
    }

    pub fn solve_image_file_with_presets(
        &self,
        path: String,
        base: SolveOptionsDto,
        presets: Vec<FovPresetDto>,
    ) -> Result<LadderOutcomeDto> {
        let (f, meta, b) = load_with_header_time(&path, base)?;
        let ps: Vec<core::FovPreset> = presets
            .into_iter()
            .map(|p| core::FovPreset {
                fov_deg: p.fov_deg,
                max_error_deg: p.max_error_deg,
            })
            .collect();
        let (mut out, attempts) = self.inner.solve_with_fov_presets(&f, &b, &ps)?;
        meta.apply_place(&mut out);
        Ok(LadderOutcomeDto {
            outcome: out.into(),
            attempts: attempts.into_iter().map(Into::into).collect(),
        })
    }

    /// Fully automatic file entry (same strategy as the C ABI's solve_image_json): header FOV
    /// hints (FITS focal length and pixel size, EXIF 35 mm focal length) first, the aspect
    /// ladder as fallback, rungs clamped to the database range; the header's observation time
    /// fills in when `base` has none. For FITS/XISF prefer ExtractionProfileDto.auto() or
    /// cleanSensor().
    pub fn solve_image_file_auto(
        &self,
        path: String,
        base: SolveOptionsDto,
    ) -> Result<LadderOutcomeDto> {
        let (f, meta) = core::imageio::load_image(&path)?;
        let presets = core::presets_with_hints(&meta, f.width, f.height);
        let mut b: core::SolveOptions = base.try_into()?;
        meta.apply_time(&mut b);
        let (mut out, attempts) = self.inner.solve_with_fov_presets(&f, &b, &presets)?;
        meta.apply_place(&mut out);
        Ok(LadderOutcomeDto {
            outcome: out.into(),
            attempts: attempts.into_iter().map(Into::into).collect(),
        })
    }

    /// Annotator. Each file is optional: a missing one leaves its layer unavailable, with the
    /// reason in `layers.reasons`. `constellationsPath` is the constellation pack
    /// (`UnisolverAssets.installConstellations()`).
    pub fn annotator(
        &self,
        dso_path: Option<String>,
        names_path: Option<String>,
        constellations_path: Option<String>,
    ) -> Result<UniAnnotator> {
        Ok(UniAnnotator {
            inner: self
                .inner
                .annotator(dso_path.as_deref(), names_path.as_deref())?
                .with_constellations(constellations_path.as_deref()),
        })
    }

    pub fn new_calibration(&self) -> Result<UniCalibration> {
        Ok(UniCalibration {
            inner: std::sync::Mutex::new(self.inner.new_calibration_session()?),
        })
    }
}

/// Pool handle: shaped like [`UniSolver`] but **without naming a database**: frames are routed
/// by FOV to a tier that can solve them, stepping across tiers on failure. Fetching and
/// installing tiers is `DbManager`'s job on the Dart side; this only uses installed tiers.
///
/// `RwLock`: solves take the read lock (concurrent), `register` after an install takes the
/// write lock (exclusive).
#[frb(opaque)]
pub struct UniSolverPool {
    inner: std::sync::RwLock<core::SolverPool>,
    /// Files that failed to open (path and reason)
    skipped: Vec<(String, String)>,
}

impl UniSolverPool {
    /// Registers every `*.db` in the directory. A broken file is skipped (see [`Self::skipped`]);
    /// it fails only when none opens, since an empty handle is harder to debug than an error.
    pub fn open_dir(dir: String) -> Result<UniSolverPool> {
        let (pool, skipped) = core::SolverPool::open_dir(&dir)?;
        Ok(Self {
            inner: std::sync::RwLock::new(pool),
            skipped,
        })
    }

    /// Empty pool: for a first launch with nothing installed; then [`Self::register`] tier by tier.
    pub fn empty() -> Result<UniSolverPool> {
        Ok(Self {
            inner: std::sync::RwLock::new(core::SolverPool::new()?),
            skipped: Vec::new(),
        })
    }

    /// Registers one more tier (after an install). Registering the same file again is idempotent.
    pub fn register(&self, db_path: String) -> Result<TierInfoDto> {
        Ok(self
            .inner
            .write()
            .map_err(|e| anyhow::anyhow!("pool lock poisoned: {e}"))?
            .register(&db_path)?
            .into())
    }

    /// Registers the narrow-field package (desktop builds): its blind index and star-tile
    /// files. Only their headers are read. Registering the same index again returns it; a
    /// second package is an error, and so is every call on iOS and Android, which have no
    /// narrow-field engine.
    pub fn register_narrow(&self, index_path: String, stars_path: String) -> Result<TierInfoDto> {
        Ok(self
            .inner
            .write()
            .map_err(|e| anyhow::anyhow!("pool lock poisoned: {e}"))?
            .register_narrow(&index_path, &stars_path)?
            .into())
    }

    /// Registered tiers: the tetra3 tiers wide to narrow, then the narrow-field engine.
    pub fn tiers(&self) -> Result<Vec<TierInfoDto>> {
        Ok(self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!("pool lock poisoned: {e}"))?
            .tiers()
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Files skipped when the directory was opened, as `"<path>: <reason>"`.
    #[frb(sync)]
    pub fn skipped(&self) -> Vec<String> {
        self.skipped
            .iter()
            .map(|(p, e)| format!("{p}: {e}"))
            .collect()
    }

    /// Fully automatic file entry without naming a tier (the one apps should use): load any
    /// of the five formats → header FOV hints (FITS, EXIF) + aspect ladder → cross-tier
    /// routing. The header's observation time fills in when `base` has none and comes back in
    /// `outcome.observationUnixMs`. HEIC is not decoded: decode it with the platform and use
    /// [`Self::solve_frame_auto`].
    pub fn solve_image_file_auto(
        &self,
        path: String,
        base: SolveOptionsDto,
    ) -> Result<PoolOutcomeDto> {
        let b: core::SolveOptions = base.try_into()?;
        Ok(self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!("pool lock poisoned: {e}"))?
            .solve_image_file_auto(&path, &b)?
            .into())
    }

    /// Camera frames and decoded photos (no file header): the aspect ladder, preceded by the
    /// FOV of `opts.focalLength35mm` when the app read one from EXIF; routed across tiers.
    pub fn solve_frame_auto(
        &self,
        frame: FrameDto,
        opts: SolveOptionsDto,
    ) -> Result<PoolOutcomeDto> {
        let f: core::Frame = frame.try_into()?;
        let o: core::SolveOptions = opts.try_into()?;
        let hints = core::aspect_ladder(f.width, f.height);
        Ok(self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!("pool lock poisoned: {e}"))?
            .solve_auto(&f, &o, &hints)?
            .into())
    }

    /// Annotator. Pass the tier that solved the frame (`PoolOutcomeDto.db`), since narrow tiers
    /// are denser; without it the widest tier is used. A narrow-field solve annotates with the
    /// narrowest tetra3 tier. The files are as in [`UniSolver::annotator`].
    pub fn annotator(
        &self,
        db: Option<String>,
        dso_path: Option<String>,
        names_path: Option<String>,
        constellations_path: Option<String>,
    ) -> Result<UniAnnotator> {
        let g = self
            .inner
            .read()
            .map_err(|e| anyhow::anyhow!("pool lock poisoned: {e}"))?;
        let solver = g
            .annotation_solver(db.as_deref())
            .ok_or_else(|| match &db {
                Some(n) => anyhow::anyhow!("no tetra3 tier in the pool to annotate {n} with"),
                None => anyhow::anyhow!("pool is empty: register a database first"),
            })?;
        Ok(UniAnnotator {
            inner: solver
                .annotator(dso_path.as_deref(), names_path.as_deref())?
                .with_constellations(constellations_path.as_deref()),
        })
    }
}

/// Unified loading of FITS/XISF/PNG/JPEG/TIFF, dispatched on magic bytes
fn load_image_luma(path: &str) -> Result<core::Frame> {
    Ok(core::imageio::load_image(path)?.0)
}

/// A file, its header and the options to solve it with: the header's observation time fills
/// in when the options have none, as in every file entry (the place goes on the outcome).
fn load_with_header_time(
    path: &str,
    opts: SolveOptionsDto,
) -> Result<(core::Frame, core::imageio::ImageMeta, core::SolveOptions)> {
    let (f, meta) = core::imageio::load_image(path)?;
    let mut o: core::SolveOptions = opts.try_into()?;
    meta.apply_time(&mut o);
    Ok((f, meta, o))
}

#[frb(opaque)]
pub struct UniAnnotator {
    inner: core::Annotator,
}

impl UniAnnotator {
    /// Languages in the names pack (empty = no names pack, English only).
    #[frb(sync)]
    pub fn languages(&self) -> Vec<String> {
        self.inner.languages()
    }

    pub fn annotate(&self, wcs: WcsDto, opts: AnnotateOptionsDto) -> Result<AnnotationsDto> {
        let w: core::Wcs = wcs.try_into()?;
        Ok(self.inner.annotate(&w, &opts.into()).into())
    }

    /// The constellation containing J2000 `(raDeg, decDeg)`, named in `language` (codes as
    /// `AnnotateOptionsDto.language`). For the frame centre pass the solve's `raDeg` /
    /// `decDeg`; for a point on the image, convert it with `wcsPixelsToSky` first. Null
    /// without a constellation pack (`annotate` reports why in `layers.reasons`).
    #[frb(sync)]
    pub fn constellation_at(
        &self,
        ra_deg: f64,
        dec_deg: f64,
        language: String,
    ) -> Result<Option<ConstellationNameDto>> {
        anyhow::ensure!(
            ra_deg.is_finite() && dec_deg.is_finite(),
            "position ({ra_deg}, {dec_deg}) is not finite"
        );
        Ok(self
            .inner
            .constellation_at(ra_deg, dec_deg, &language)
            .map(Into::into))
    }
}

#[frb(opaque)]
pub struct UniCalibration {
    inner: std::sync::Mutex<core::CalibrationSession>,
}

impl UniCalibration {
    pub fn add_image_file(&self, path: String, opts: SolveOptionsDto) -> Result<SolveOutcomeDto> {
        let f = load_image_luma(&path)?;
        let o: core::SolveOptions = opts.try_into()?;
        Ok(self.inner.lock().unwrap().add_frame(&f, &o)?.into())
    }

    #[frb(sync)]
    pub fn count(&self) -> u32 {
        self.inner.lock().unwrap().count() as u32
    }

    pub fn fit(&self, model: CalibModelDto) -> Result<CalibrationReportDto> {
        let m = match model {
            CalibModelDto::Radial => core::CalibModel::Radial,
            CalibModelDto::Polynomial { order } => core::CalibModel::Polynomial { order },
        };
        Ok(self.inner.lock().unwrap().fit(m)?.into())
    }
}

pub fn camera_params_to_json(c: CameraParamsDto) -> Result<String> {
    let core_c: core::CameraParams = c.try_into()?;
    Ok(serde_json::to_string(&core_c)?)
}

pub fn camera_params_from_json(j: String) -> Result<CameraParamsDto> {
    let core_c: core::CameraParams = serde_json::from_str(&j)?;
    Ok(core_c.into())
}

/// For Dart integration tests only: checks that a panic becomes an exception (FRB catches
/// the unwind and raises it in Dart)
pub fn debug_trigger_panic() {
    panic!("frb panic propagation test")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db_path() -> String {
        unisolver_synth::test_db_file("unisolver_frb_test.db")
    }

    #[test]
    fn solve_frame_end_to_end_via_dto() {
        let s = UniSolver::new(test_db_path()).unwrap();
        let q = unisolver_synth::look_at(120.0, 40.0, 15.0);
        let img = unisolver_synth::render(
            unisolver_synth::test_db().star_catalog.stars(),
            &q,
            20.0,
            1024,
            768,
            &unisolver_synth::RenderParams::default(),
            5,
        );
        let bytes: Vec<u8> = img.iter().flat_map(|v| v.to_le_bytes()).collect();
        let frame = FrameDto {
            width: 1024,
            height: 768,
            row_stride_bytes: None,
            kind: PixelKindDto::LumaF32,
            bytes,
        };
        let mut opts = SolveOptionsDto::defaults(20.0);
        opts.profile = ExtractionProfileDto::CleanSensor;
        let out = s.solve_frame(frame, opts).unwrap();
        assert!(matches!(out.status, SolveStatusDto::Ok));
        let g = out.solution.unwrap();
        assert!((g.ra_deg - 120.0).abs() < 0.1 && (g.dec_deg - 40.0).abs() < 0.1);
        // annotate round-trips through WcsDto
        let wcs: core::Wcs = g.wcs.try_into().unwrap();
        let ann = s.annotator(None, None, None).unwrap();
        let a = ann
            .annotate(wcs.clone().into(), AnnotateOptionsDto::defaults())
            .unwrap();
        assert!(a.layers.catalog_stars && a.stars.len() > 5);
        // The frame centre's constellation, from the bundled pack; none without it
        assert!(ann
            .constellation_at(g.ra_deg, g.dec_deg, "en".into())
            .unwrap()
            .is_none());
        let pack = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/unisolver_constellations.bin"
        );
        let ann = s.annotator(None, None, Some(pack.into())).unwrap();
        let c = ann
            .constellation_at(g.ra_deg, g.dec_deg, "en".into())
            .unwrap()
            .expect("pack loaded");
        assert_eq!((c.abbr.as_str(), c.name.as_str()), ("Lyn", "Lynx"));
        assert!(ann.constellation_at(f64::NAN, 0.0, "en".into()).is_err());
        // Constellation art comes back as a mesh with NaN for unplaced vertices
        let mut opts = AnnotateOptionsDto::defaults();
        opts.include_constellations = true;
        opts.constellation_art = true;
        let a = ann.annotate(wcs.into(), opts).unwrap();
        let art = a
            .constellations
            .iter()
            .find_map(|c| c.art.as_ref())
            .expect("a constellation with art in a 20° field at (120°, +40°)");
        assert_eq!((art.cols, art.rows, art.points.len()), (16, 16, 512));
        assert!(art.points.iter().any(|v| v.is_finite()));
    }

    /// The pool handle along the path Dart takes: open_dir → tiers → solve a frame → annotator from the tier that solved it.
    #[test]
    fn pool_handle_routes_and_annotates_via_dto() {
        let dir = std::env::temp_dir().join("frb_pool_test");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, db) in [
            ("unisolver_15_40", unisolver_synth::test_db()),
            ("unisolver_8_15", unisolver_synth::narrow_test_db()),
        ] {
            let dst = dir.join(format!("{name}.db"));
            let tmp = dir.join(format!("{name}.db.tmp{}", std::process::id()));
            db.save_to_file_v2(tmp.to_str().unwrap()).unwrap();
            std::fs::rename(&tmp, &dst).unwrap();
        }
        let pool = UniSolverPool::open_dir(dir.to_str().unwrap().to_string()).unwrap();
        assert!(pool.skipped().is_empty());
        let tiers = pool.tiers().unwrap();
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, "unisolver_15_40", "widest first");

        // A 10° field from the narrow catalog with no hint: the pool must sweep to the narrow tier itself
        let q = unisolver_synth::look_at(250.0, -20.0, 15.0);
        let img = unisolver_synth::render(
            unisolver_synth::narrow_test_db().star_catalog.stars(),
            &q,
            10.0,
            1024,
            768,
            &unisolver_synth::RenderParams::default(),
            5,
        );
        let frame = FrameDto {
            width: 1024,
            height: 768,
            row_stride_bytes: None,
            kind: PixelKindDto::LumaF32,
            bytes: img.iter().flat_map(|v| v.to_le_bytes()).collect(),
        };
        let mut opts = SolveOptionsDto::defaults(20.0);
        opts.profile = ExtractionProfileDto::CleanSensor;
        opts.timeout_ms = Some(20_000);
        let r = pool.solve_frame_auto(frame, opts).unwrap();
        assert!(
            matches!(r.outcome.status, SolveStatusDto::Ok),
            "attempts: {:?}",
            r.attempts
                .iter()
                .map(|a| format!("{}@{:.1}", a.db, a.fov_deg))
                .collect::<Vec<_>>()
        );
        assert_eq!(r.db.as_deref(), Some("unisolver_8_15"));
        assert_eq!(r.extract_count, 1, "extraction must be reused across tiers");
        let g = r.outcome.solution.unwrap();
        assert!((g.ra_deg - 250.0).abs() < 0.1 && (g.dec_deg + 20.0).abs() < 0.1);

        // Annotate with the tier that solved it (narrow tiers are denser)
        let ann = pool.annotator(r.db.clone(), None, None, None).unwrap();
        let a = ann.annotate(g.wcs, AnnotateOptionsDto::defaults()).unwrap();
        assert!(a.layers.catalog_stars && a.stars.len() > 5);
        // Registration is idempotent
        let info = pool
            .register(dir.join("unisolver_8_15.db").to_str().unwrap().to_string())
            .unwrap();
        assert_eq!(info.name, "unisolver_8_15");
        assert_eq!(pool.tiers().unwrap().len(), 2);
    }

    #[test]
    fn json_camera_roundtrip() {
        let dto: CameraParamsDto = core::CameraParams::from_horizontal_fov(45.0, 720, 1280)
            .unwrap()
            .into();
        let j = camera_params_to_json(dto.clone()).unwrap();
        let back = camera_params_from_json(j).unwrap();
        assert!((back.focal_length_px - dto.focal_length_px).abs() < 1e-9);
    }
}
