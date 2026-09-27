//! unisolver C ABI: opaque handles, JSON string results and explicit free functions,
//! for non-Flutter consumers (INDI/ASCOM drivers, native desktop apps). All logic lives
//! in unisolver-core; this layer only marshals.
use std::ffi::{c_char, CStr, CString};
use std::ptr;

use unisolver_core as core;

/// Opaque solver handle. Release with [`unisolver_close`].
pub struct UnisolverSolver {
    solver: core::Solver,
}

fn set_error(error_out: *mut *mut c_char, msg: &str) {
    if !error_out.is_null() {
        let c = CString::new(msg.replace('\0', "?")).unwrap_or_default();
        unsafe { *error_out = c.into_raw() };
    }
}

fn clear_error(error_out: *mut *mut c_char) {
    if !error_out.is_null() {
        unsafe { *error_out = ptr::null_mut() };
    }
}

unsafe fn cstr<'a>(p: *const c_char, what: &str) -> Result<&'a str, String> {
    if p.is_null() {
        return Err(format!("{what} is NULL"));
    }
    CStr::from_ptr(p)
        .to_str()
        .map_err(|_| format!("{what} is not valid UTF-8"))
}

fn to_owned_cstring(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "?"))
        .unwrap_or_default()
        .into_raw()
}

/// Library version (static string; do not free).
#[no_mangle]
pub extern "C" fn unisolver_version() -> *const c_char {
    concat!("unisolver ", env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Attributions for the data the engine ships or reads, as an OWNED JSON array (free
/// it with unisolver_string_free). Each entry has `id`, `name`, `applies_to`, `license`,
/// `text` and `url`; show the `text` of the sources your app uses (Gaia DR3 requires it).
#[no_mangle]
pub extern "C" fn unisolver_attributions_json() -> *mut c_char {
    let json = serde_json::to_string(core::data_attributions()).unwrap_or_else(|_| "[]".into());
    to_owned_cstring(json)
}

/// Opens a solver database. On failure returns NULL and sets `error_out` (free it with
/// unisolver_string_free).
///
/// # Safety
/// `db_path` must be a valid NUL-terminated string; `error_out` is NULL or a writable pointer slot.
#[no_mangle]
pub unsafe extern "C" fn unisolver_open(
    db_path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut UnisolverSolver {
    clear_error(error_out);
    let path = match cstr(db_path, "db_path") {
        Ok(p) => p,
        Err(e) => {
            set_error(error_out, &e);
            return ptr::null_mut();
        }
    };
    match core::Solver::from_file(path) {
        Ok(solver) => Box::into_raw(Box::new(UnisolverSolver { solver })),
        Err(e) => {
            set_error(error_out, &e.to_string());
            ptr::null_mut()
        }
    }
}

/// Releases a solver. Accepts NULL.
///
/// # Safety
/// `solver` must come from unisolver_open and not be freed yet; no call may be in flight on it.
#[no_mangle]
pub unsafe extern "C" fn unisolver_close(solver: *mut UnisolverSolver) {
    if !solver.is_null() {
        drop(Box::from_raw(solver));
    }
}

#[derive(serde::Serialize)]
struct SolveJson {
    status: String,
    extraction_retried: bool,
    num_centroids: usize,
    /// Star-shape diagnostic (median elongation of the brightest quarter; relative: compare
    /// with a baseline from the same camera)
    median_elongation: Option<f32>,
    /// Name of the tier that solved it (pool entries only)
    #[serde(skip_serializing_if = "Option::is_none")]
    db: Option<String>,
    /// Observation time the solve used (Unix ms, UTC): `observation_unix_ms` from the
    /// options, or for files the header's (FITS DATE-OBS, EXIF with a zone). Pass it to
    /// annotation for the solar-system layer; null when neither gave one
    observation_unix_ms: Option<i64>,
    /// Where the photo was taken, for files whose EXIF has a GPS position
    /// (`{lat_deg, lon_deg, alt_m}`): pass it to annotation with the time (the moon's
    /// parallax); null otherwise
    observer: Option<core::Observer>,
    attempts: Vec<AttemptJson>,
    solution: Option<SolutionJson>,
}

#[derive(serde::Serialize)]
struct AttemptJson {
    fov_deg: f32,
    status: String,
    solve_ms: f32,
    /// Tier used for this attempt (pool entries only)
    #[serde(skip_serializing_if = "Option::is_none")]
    db: Option<String>,
}

#[derive(serde::Serialize)]
struct SolutionJson {
    ra_deg: f64,
    dec_deg: f64,
    roll_deg: f64,
    fov_deg: f32,
    num_matches: u32,
    rmse_arcsec: f32,
    prob: f64,
    scale_arcsec_per_px: f64,
    wcs: core::Wcs,
}

/// Solve options as JSON. **Every field is optional** (`{}` or NULL keeps the defaults).
///
/// Giving `fov_deg` or `camera` means "FOV known": **no ladder**, just that rung. Telescope
/// drivers usually know their focal length and pixel size; with a known FOV one telescope
/// camera solved in 35 ms versus 425 ms walking the ladder.
#[derive(serde::Deserialize)]
#[serde(default)]
struct SolveOptsJson {
    /// Known horizontal FOV (degrees, along width). Skips the ladder
    fov_deg: Option<f32>,
    /// FOV tolerance (degrees); defaults to 12% of fov_deg
    fov_max_error_deg: Option<f32>,
    /// Calibrated camera (pass back the `camera` from unisolver_calibration_fit_json unchanged)
    camera: Option<core::CameraParams>,
    /// Tracking: the previous frame's attitude [w,x,y,z]. **Needs fov_deg or camera too**
    /// (tracking needs the scale)
    attitude_hint_wxyz: Option<[f32; 4]>,
    hint_uncertainty_deg: f32,
    /// true = no blind-solve fallback when the hint fails
    strict_hint: bool,
    /// Extraction profile: `auto` (default) / `phone` (σ10) / `clean` (σ5)
    profile: String,
    /// Explicit σ threshold; selects the custom profile (overrides `profile`), as solvecli --sigma
    sigma: Option<f32>,
    max_centroids: usize,
    retry_alternate_profile: bool,
    /// Ladders only: after the staged search fails, search exhaustively (slow failures)
    thorough: bool,
    match_threshold: f64,
    timeout_ms: Option<u64>,
    /// Aberration: observation time (Unix ms)
    observation_unix_ms: Option<i64>,
    /// Advanced override: observer ICRS velocity in km/s
    observer_velocity_km_s: Option<[f64; 3]>,
    /// Ladders only: EXIF FocalLengthIn35mmFilm read by the caller, for formats the engine
    /// does not decode (HEIC: decode with the platform, then use a frame entry). Its FOV is
    /// tried first as a hint (±15%) and the ladder still follows; ignored when `fov_deg` or
    /// `camera` is given. Files read their own EXIF
    focal_length_35mm: Option<f32>,
}

impl Default for SolveOptsJson {
    fn default() -> Self {
        let d = core::SolveOptions::new(70.0);
        Self {
            fov_deg: None,
            fov_max_error_deg: None,
            camera: None,
            attitude_hint_wxyz: None,
            hint_uncertainty_deg: d.hint_uncertainty_deg,
            strict_hint: d.strict_hint,
            profile: "auto".into(),
            sigma: None,
            max_centroids: 100,
            retry_alternate_profile: d.retry_alternate_profile,
            thorough: d.thorough,
            match_threshold: d.match_threshold,
            // The C entries' existing default, unchanged
            timeout_ms: Some(4000),
            observation_unix_ms: None,
            observer_velocity_km_s: None,
            focal_length_35mm: None,
        }
    }
}

impl SolveOptsJson {
    fn parse(opts_json: *const c_char) -> Result<Self, String> {
        if opts_json.is_null() {
            return Ok(Self::default());
        }
        let text = unsafe { cstr(opts_json, "opts_json") }?;
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(text).map_err(|e| format!("opts_json: {e}"))
    }

    /// → (core options, whether the FOV is known (no ladder)). Consumes self: camera and TLE move out
    fn into_core(self) -> Result<(core::SolveOptions, bool), String> {
        let known = self.fov_deg.is_some() || self.camera.is_some();
        if self.attitude_hint_wxyz.is_some() && !known {
            return Err(
                "attitude_hint_wxyz needs fov_deg or camera (tracking needs the scale)".into(),
            );
        }
        // With a camera, fov_estimate only passes validation; the real value depends on the
        // **image width**, unknown here, so keep 70 and let the solve_*_with_opts callers rewrite it
        let mut o = core::SolveOptions::new(self.fov_deg.unwrap_or(70.0));
        o.camera = self.camera;
        o.fov_max_error_deg = self
            .fov_max_error_deg
            .or_else(|| self.fov_deg.map(|f| (f * 0.12).max(0.3)));
        o.attitude_hint = self.attitude_hint_wxyz;
        o.hint_uncertainty_deg = self.hint_uncertainty_deg;
        o.strict_hint = self.strict_hint;
        o.extraction = match self.sigma {
            Some(sig) => core::ExtractionProfile::Custom(core::ExtractionOptions::Ccl {
                sigma_threshold: sig,
                max_centroids: self.max_centroids,
            }),
            None => match self.profile.as_str() {
                "clean" => core::ExtractionProfile::CleanSensor,
                "phone" => core::ExtractionProfile::PhoneJpeg,
                "auto" => core::ExtractionProfile::Auto,
                other => return Err(format!("unknown profile '{other}' (auto|phone|clean)")),
            },
        };
        o.retry_alternate_profile = self.retry_alternate_profile;
        o.thorough = self.thorough;
        o.match_threshold = self.match_threshold;
        o.timeout_ms = self.timeout_ms;
        o.observation_unix_ms = self.observation_unix_ms;
        o.observer_velocity_km_s = self.observer_velocity_km_s;
        o.focal_length_35mm = self.focal_length_35mm;
        Ok((o, known))
    }
}

fn attempt_json(a: &core::FovAttempt, db: Option<&str>) -> AttemptJson {
    AttemptJson {
        fov_deg: a.fov_deg,
        status: format!("{:?}", a.status),
        solve_ms: a.solve_ms,
        db: db.map(str::to_string),
    }
}

/// Solve result → public JSON (shared by single and pool entries; pools add `db`)
fn build_solve_json(
    out: core::SolveOutcome,
    attempts: Vec<AttemptJson>,
    db: Option<String>,
) -> SolveJson {
    SolveJson {
        status: format!("{:?}", out.status),
        extraction_retried: out.extraction_retried,
        num_centroids: out.centroids.len(),
        median_elongation: out.median_elongation,
        db,
        observation_unix_ms: out.observation_unix_ms,
        observer: out.observer,
        attempts,
        solution: out.solution.map(|g| SolutionJson {
            ra_deg: g.ra_deg,
            dec_deg: g.dec_deg,
            roll_deg: g.roll_deg,
            fov_deg: g.fov_deg,
            num_matches: g.num_matches,
            rmse_arcsec: g.rmse_arcsec,
            prob: g.prob,
            scale_arcsec_per_px: g.wcs.scale_arcsec_per_px(),
            wcs: g.wcs,
        }),
    }
}

/// With a camera, fov_estimate only passes validation; the real value comes from the image width
fn fit_camera_fov(base: &mut core::SolveOptions, width: u32) {
    if let Some(c) = base.camera.clone() {
        base.fov_estimate_deg = c.horizontal_fov_deg(width) as f32;
        base.fov_max_error_deg = None;
    }
}

/// Shared single-database solve: a known FOV hits one rung, otherwise the given ladder.
/// File and frame entries differ only in where the ladder comes from (header hints vs aspect).
fn solve_frame_with_opts(
    solver: &core::Solver,
    frame: &core::Frame,
    base: &core::SolveOptions,
    known: bool,
    presets: &[core::FovPreset],
    header: Option<&core::imageio::ImageMeta>,
) -> Result<String, String> {
    let (mut out, attempts) = if known {
        let fov = base.fov_estimate_deg;
        let out = solver.solve(frame, base).map_err(|e| e.to_string())?;
        let a = vec![AttemptJson {
            fov_deg: fov,
            status: format!("{:?}", out.status),
            solve_ms: out.timing.solve_ms,
            db: None,
        }];
        (out, a)
    } else {
        let (out, a) = solver
            .solve_with_fov_presets(frame, base, presets)
            .map_err(|e| e.to_string())?;
        let a = a.iter().map(|x| attempt_json(x, None)).collect();
        (out, a)
    };
    if let Some(m) = header {
        m.apply_place(&mut out);
    }
    serde_json::to_string(&build_solve_json(out, attempts, None)).map_err(|e| e.to_string())
}

fn solve_file_with_opts(
    solver: &core::Solver,
    path: &str,
    opts: SolveOptsJson,
) -> Result<String, String> {
    let (frame, meta) = core::imageio::load_image(path).map_err(|e| e.to_string())?;
    let (mut base, known) = opts.into_core()?;
    fit_camera_fov(&mut base, frame.width);
    meta.apply_time(&mut base);
    let presets = core::presets_with_hints(&meta, frame.width, frame.height);
    solve_frame_with_opts(solver, &frame, &base, known, &presets, Some(&meta))
}

/// Shared pool solve: with a known FOV only tiers covering it are tried, otherwise the ladder is dispatched.
fn pool_solve_frame_with_opts(
    pool: &core::SolverPool,
    frame: &core::Frame,
    base: &core::SolveOptions,
    hints: &[core::FovPreset],
    header: Option<&core::imageio::ImageMeta>,
) -> Result<String, String> {
    let mut r = pool
        .solve_auto(frame, base, hints)
        .map_err(|e| e.to_string())?;
    if let Some(m) = header {
        m.apply_place(&mut r.outcome);
    }
    let attempts = r
        .attempts
        .iter()
        .map(|a| AttemptJson {
            fov_deg: a.fov_deg,
            status: format!("{:?}", a.status),
            solve_ms: a.solve_ms,
            db: Some(a.db.clone()),
        })
        .collect();
    serde_json::to_string(&build_solve_json(r.outcome, attempts, r.db)).map_err(|e| e.to_string())
}

/// One rung when the FOV is known; otherwise the caller's fallback ladder (header hints for
/// files, aspect ladder for raw frames)
fn hints_for(
    fov_deg: Option<f32>,
    base: &core::SolveOptions,
    fallback: Vec<core::FovPreset>,
) -> Vec<core::FovPreset> {
    match fov_deg {
        Some(f) => vec![core::FovPreset {
            fov_deg: f,
            max_error_deg: base.fov_max_error_deg.unwrap_or((f * 0.12).max(0.3)),
        }],
        None => fallback,
    }
}

fn pool_solve_file_with_opts(
    pool: &core::SolverPool,
    path: &str,
    opts: SolveOptsJson,
) -> Result<String, String> {
    let (frame, meta) = core::imageio::load_image(path).map_err(|e| e.to_string())?;
    let fov_deg = opts.fov_deg;
    let (mut base, _) = opts.into_core()?;
    fit_camera_fov(&mut base, frame.width);
    meta.apply_time(&mut base);
    let fallback = core::presets_with_hints(&meta, frame.width, frame.height);
    let hints = hints_for(fov_deg, &base, fallback);
    pool_solve_frame_with_opts(pool, &frame, &base, &hints, Some(&meta))
}

/// Raw pixel buffer → `core::Frame`. Multi-byte samples are **native-endian** (every target
/// is little-endian), and the buffer is **copied**: a C caller's buffer usually comes from a
/// camera callback and cannot be assumed to outlive the solve. Size and stride are validated
/// once, by core's `to_luma_f32`.
unsafe fn frame_from_raw(
    pixels: *const u8,
    len: usize,
    width: u32,
    height: u32,
    kind: *const c_char,
    row_stride_bytes: u32,
) -> Result<core::Frame, String> {
    if pixels.is_null() {
        return Err("pixels is NULL".into());
    }
    let kind = cstr(kind, "kind")?;
    let bytes = std::slice::from_raw_parts(pixels, len);
    let pixel_data = match kind.to_ascii_lowercase().as_str() {
        "luma8" => core::PixelData::Luma8(bytes.to_vec()),
        "luma16" => core::PixelData::Luma16(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_ne_bytes)
                .collect(),
        ),
        "luma_f32" | "lumaf32" => core::PixelData::LumaF32(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .copied()
                .map(f32::from_ne_bytes)
                .collect(),
        ),
        "rgba8" => core::PixelData::Rgba8(bytes.to_vec()),
        other => {
            return Err(format!(
                "unknown kind '{other}' (luma8|luma16|luma_f32|rgba8)"
            ))
        }
    };
    Ok(core::Frame {
        width,
        height,
        row_stride_bytes: (row_stride_bytes > 0).then_some(row_stride_bytes),
        pixels: pixel_data,
    })
}

/// Solves an image file (FITS/XISF/PNG/JPEG/TIFF detected automatically; a header FOV is
/// only a hint, the aspect ladder follows on failure). Returns OWNED JSON (free with
/// unisolver_string_free).
///
/// To pass options (known FOV / calibrated camera / tracking hint / profile / timeout) use
/// [`unisolver_solve_image_json_opts`].
///
/// # Safety
/// `path` is a valid NUL-terminated string; `solver` is live; `error_out` as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_solve_image_json(
    solver: *const UnisolverSolver,
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let solver = solver.as_ref().ok_or("solver is NULL")?;
        solve_file_with_opts(
            &solver.solver,
            cstr(path, "path")?,
            SolveOptsJson::default(),
        )
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Solve with options: `opts_json` per [`SolveOptsJson`] (NULL or `{}` is the same as
/// [`unisolver_solve_image_json`]). Known FOV, calibrated camera, tracking hint, extraction
/// profile and timeout all come in here.
///
/// **Tracking** (burst capture, guiding, centring loops): put the previous frame's attitude
/// quaternion in `attitude_hint_wxyz` and give `fov_deg` or `camera`. The solve skips the
/// 4-star hash search and matches catalog stars projected around the hint: 3 stars suffice
/// (a blind solve needs 4). With `strict_hint` false a failure falls back to a blind solve.
///
/// # Safety
/// `solver` is live; `path` is a valid NUL-terminated string; `opts_json` is NULL or a valid
/// NUL-terminated string; `error_out` as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_solve_image_json_opts(
    solver: *const UnisolverSolver,
    path: *const c_char,
    opts_json: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let solver = solver.as_ref().ok_or("solver is NULL")?;
        let opts = SolveOptsJson::parse(opts_json)?;
        solve_file_with_opts(&solver.solver, cstr(path, "path")?, opts)
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// **Direct frame input**: solves pixels in memory without touching disk. Use it for camera
/// callbacks, video streams and already-decoded frames instead of writing a temporary file.
///
/// - `pixels` / `len`: the pixel buffer and its length in bytes. **It is copied**: a camera
///   callback's buffer is usually reclaimed at once and cannot be assumed to outlive the solve.
/// - `kind`: `luma8` / `luma16` / `luma_f32` / `rgba8`; multi-byte samples are **native-endian**.
/// - `row_stride_bytes`: bytes per source row, for padded buffers (Android `YUV_420_888`
///   Y-plane rowStride, iOS `bytesPerRow`); 0 when tightly packed.
/// - `opts_json`: as [`unisolver_solve_image_json_opts`]. A raw frame has **no header**, so
///   without `fov_deg`/`camera` the fallback is the aspect ladder, not header hints; a photo
///   decoded by the platform (HEIC) passes its EXIF as `focal_length_35mm` (tried first) and
///   `observation_unix_ms`. Live and tracking use should pass `fov_deg` and
///   `attitude_hint_wxyz` anyway.
///
/// # Safety
/// `solver` is live; `pixels` points to at least `len` readable bytes; `kind` is a valid
/// NUL-terminated string; `opts_json` is NULL or a valid NUL-terminated string.
#[no_mangle]
#[allow(clippy::too_many_arguments)] // a frame needs these; a struct would need a fixed ABI layout
pub unsafe extern "C" fn unisolver_solve_frame_json_opts(
    solver: *const UnisolverSolver,
    pixels: *const u8,
    len: usize,
    width: u32,
    height: u32,
    kind: *const c_char,
    row_stride_bytes: u32,
    opts_json: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let solver = solver.as_ref().ok_or("solver is NULL")?;
        let frame = frame_from_raw(pixels, len, width, height, kind, row_stride_bytes)?;
        let (mut base, known) = SolveOptsJson::parse(opts_json)?.into_core()?;
        fit_camera_fov(&mut base, frame.width);
        let presets = core::aspect_ladder(frame.width, frame.height);
        solve_frame_with_opts(&solver.solver, &frame, &base, known, &presets, None)
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Opaque pool handle. Release with [`unisolver_pool_close`].
pub struct UnisolverPool {
    pool: core::SolverPool,
    /// Files that failed to open (path and reason), reported by unisolver_pool_tiers_json
    skipped: Vec<(String, String)>,
}

#[derive(serde::Serialize)]
struct TiersJson {
    tiers: Vec<TierJson>,
    /// Files in the directory that failed to open; empty when all is well
    skipped: Vec<SkippedJson>,
}

#[derive(serde::Serialize)]
struct TierJson {
    name: String,
    path: String,
    min_fov_deg: f32,
    max_fov_deg: f32,
    num_stars: u64,
    num_patterns: u32,
    star_max_magnitude: f32,
}

#[derive(serde::Serialize)]
struct SkippedJson {
    path: String,
    error: String,
}

fn tier_json(t: &core::TierInfo) -> TierJson {
    TierJson {
        name: t.name.clone(),
        path: t.path.clone(),
        min_fov_deg: t.min_fov_deg,
        max_fov_deg: t.max_fov_deg,
        num_stars: t.num_stars,
        num_patterns: t.num_patterns,
        star_max_magnitude: t.star_max_magnitude,
    }
}

/// Opens a pool, registering every `*.db` in `dir`. A file that fails to open is only
/// recorded in `skipped` (see [`unisolver_pool_tiers_json`]); it fails only when **none** opens.
///
/// # Safety
/// `dir` must be a valid NUL-terminated string; `error_out` is NULL or a writable pointer slot.
#[no_mangle]
pub unsafe extern "C" fn unisolver_pool_open(
    dir: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut UnisolverPool {
    clear_error(error_out);
    let dir = match cstr(dir, "dir") {
        Ok(p) => p,
        Err(e) => {
            set_error(error_out, &e);
            return ptr::null_mut();
        }
    };
    match core::SolverPool::open_dir(dir) {
        Ok((pool, skipped)) => Box::into_raw(Box::new(UnisolverPool { pool, skipped })),
        Err(e) => {
            set_error(error_out, &e.to_string());
            ptr::null_mut()
        }
    }
}

/// Registers one more tier (call it after a download finishes). Returns the tier as OWNED
/// JSON. Registering the same file again is idempotent.
///
/// # Safety
/// `pool` is live and **no other call may be in flight** (this call changes the pool);
/// `db_path` is a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn unisolver_pool_register(
    pool: *mut UnisolverPool,
    db_path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let pool = pool.as_mut().ok_or("pool is NULL")?;
        let path = cstr(db_path, "db_path")?;
        let info = pool.pool.register(path).map_err(|e| e.to_string())?;
        serde_json::to_string(&tier_json(&info)).map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Registered tiers plus the files skipped at open, as OWNED JSON.
///
/// # Safety
/// `pool` is live; `error_out` as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_pool_tiers_json(
    pool: *const UnisolverPool,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let pool = pool.as_ref().ok_or("pool is NULL")?;
        let j = TiersJson {
            tiers: pool.pool.tiers().iter().map(tier_json).collect(),
            skipped: pool
                .skipped
                .iter()
                .map(|(p, e)| SkippedJson {
                    path: p.clone(),
                    error: e.clone(),
                })
                .collect(),
        };
        serde_json::to_string(&j).map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Solves an image file without naming a tier: header FOV hints first, the aspect ladder as
/// fallback, rungs dispatched by tier range, then a range sweep for tiers never reached.
/// Returns OWNED JSON with an extra `db` field (the tier that solved it).
///
/// # Safety
/// `path` is a valid NUL-terminated string; `pool` is live; `error_out` as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_pool_solve_image_json(
    pool: *const UnisolverPool,
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let pool = pool.as_ref().ok_or("pool is NULL")?;
        pool_solve_file_with_opts(&pool.pool, cstr(path, "path")?, SolveOptsJson::default())
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Pool solve with options: `opts_json` as [`unisolver_solve_image_json_opts`]. With
/// `fov_deg` or `camera`, only tiers covering that FOV are tried; no ladder.
///
/// # Safety
/// `pool` is live; `path` is a valid NUL-terminated string; `opts_json` is NULL or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn unisolver_pool_solve_image_json_opts(
    pool: *const UnisolverPool,
    path: *const c_char,
    opts_json: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let pool = pool.as_ref().ok_or("pool is NULL")?;
        let opts = SolveOptsJson::parse(opts_json)?;
        pool_solve_file_with_opts(&pool.pool, cstr(path, "path")?, opts)
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Direct frame input with pool routing. Parameters as [`unisolver_solve_frame_json_opts`];
/// the JSON adds `db` (the tier that solved it).
///
/// # Safety
/// As [`unisolver_solve_frame_json_opts`], with `pool` live.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn unisolver_pool_solve_frame_json_opts(
    pool: *const UnisolverPool,
    pixels: *const u8,
    len: usize,
    width: u32,
    height: u32,
    kind: *const c_char,
    row_stride_bytes: u32,
    opts_json: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let pool = pool.as_ref().ok_or("pool is NULL")?;
        let frame = frame_from_raw(pixels, len, width, height, kind, row_stride_bytes)?;
        let opts = SolveOptsJson::parse(opts_json)?;
        let fov_deg = opts.fov_deg;
        let (mut base, _) = opts.into_core()?;
        fit_camera_fov(&mut base, frame.width);
        let fallback = core::aspect_ladder(frame.width, frame.height);
        let hints = hints_for(fov_deg, &base, fallback);
        pool_solve_frame_with_opts(&pool.pool, &frame, &base, &hints, None)
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Releases a pool. Accepts NULL.
///
/// # Safety
/// `pool` must come from unisolver_pool_open and not be freed yet; no call may be in flight on it.
#[no_mangle]
pub unsafe extern "C" fn unisolver_pool_close(pool: *mut UnisolverPool) {
    if !pool.is_null() {
        drop(Box::from_raw(pool));
    }
}

#[derive(serde::Serialize)]
struct SatelliteJson {
    name: String,
    ra_deg: f64,
    dec_deg: f64,
    range_km: f64,
    /// Below the horizon means hidden by the Earth: do not draw
    above_horizon: bool,
}

/// Topocentric satellite positions (TLE + SGP4). `tle_text` holds two- or three-line sets,
/// **fetched by the caller** (this library never goes online). Returns an OWNED JSON array;
/// empty text gives `[]`, while text with no parsable set is an error (an empty array would
/// read as "no passes").
///
/// # Safety
/// `tle_text` must be a valid NUL-terminated string; `error_out` is NULL or a writable pointer slot.
#[no_mangle]
pub unsafe extern "C" fn unisolver_satellites_json(
    tle_text: *const c_char,
    unix_ms: i64,
    lat_deg: f64,
    lon_deg: f64,
    alt_m: f64,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let tle = cstr(tle_text, "tle_text")?;
        let obs = core::Observer {
            lat_deg,
            lon_deg,
            alt_m,
        };
        obs.validate().map_err(|e| e.to_string())?;
        let sats = core::satellites::satellite_positions(tle, unix_ms, &obs)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|s| SatelliteJson {
                name: s.name,
                ra_deg: s.ra_deg,
                dec_deg: s.dec_deg,
                range_km: s.range_km,
                above_horizon: s.above_horizon,
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&sats).map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Opaque annotator handle. Release with [`unisolver_annotator_close`].
///
/// **Build once and keep it**: construction reads and parses the DSO catalog (771 KB) and the
/// names pack (215 KB) in about 2.1 ms, while annotating a frame takes about 1 ms; rebuilding
/// per solve triples the cost.
pub struct UnisolverAnnotator {
    inner: core::Annotator,
}

/// Annotation options as JSON. **Every field is optional** (`{}` means all defaults).
/// Mirrors `core::AnnotateOptions`, defined separately so missing fields are legal.
#[derive(serde::Deserialize)]
#[serde(default)]
struct AnnotateOptsJson {
    star_max_mag: Option<f32>,
    max_stars: usize,
    include_star_names: bool,
    include_dso: bool,
    dso_max_mag: Option<f32>,
    /// Projected outlines of extended objects (`objects[].outlines`)
    dso_outlines: bool,
    /// Highest outline level: 1 = faint outer edge … 3 = bright core
    max_outline_level: u8,
    include_solar_system: bool,
    observation_unix_ms: Option<i64>,
    observer: Option<core::Observer>,
    satellite_tle: Option<String>,
    language: String,
    /// Constellation figures with their names (`constellations`); needs the constellation pack
    include_constellations: bool,
    /// IAU constellation boundaries (`boundaries`); needs the constellation pack
    constellation_boundaries: bool,
    /// Equatorial grid (`grid`, J2000 right ascension and declination)
    equatorial_grid: bool,
    /// Horizontal grid (`grid`, apparent altitude and azimuth); needs `observation_unix_ms`
    /// and `observer`
    horizontal_grid: bool,
    /// Screen pixels between grid lines (default 150)
    grid_spacing_px: Option<f64>,
    /// What the app shows: `{x, y, width, height}` of the visible image region in image
    /// pixels and `scale` (screen pixels per image pixel). Lines and labels follow it
    viewport: Option<core::Viewport>,
}

impl Default for AnnotateOptsJson {
    fn default() -> Self {
        let d = core::AnnotateOptions::default();
        Self {
            star_max_mag: d.star_max_mag,
            max_stars: d.max_stars,
            include_star_names: d.include_star_names,
            include_dso: d.include_dso,
            dso_max_mag: d.dso_max_mag,
            dso_outlines: d.dso_outlines,
            max_outline_level: d.max_outline_level,
            include_solar_system: d.include_solar_system,
            observation_unix_ms: d.observation_unix_ms,
            observer: d.observer,
            satellite_tle: d.satellite_tle,
            language: d.language,
            include_constellations: d.include_constellations,
            constellation_boundaries: d.constellation_boundaries,
            equatorial_grid: d.equatorial_grid,
            horizontal_grid: d.horizontal_grid,
            grid_spacing_px: d.grid_spacing_px,
            viewport: d.viewport,
        }
    }
}

impl From<AnnotateOptsJson> for core::AnnotateOptions {
    fn from(j: AnnotateOptsJson) -> Self {
        Self {
            star_max_mag: j.star_max_mag,
            max_stars: j.max_stars,
            include_star_names: j.include_star_names,
            include_dso: j.include_dso,
            dso_max_mag: j.dso_max_mag,
            dso_outlines: j.dso_outlines,
            max_outline_level: j.max_outline_level,
            include_solar_system: j.include_solar_system,
            observation_unix_ms: j.observation_unix_ms,
            observer: j.observer,
            satellite_tle: j.satellite_tle,
            language: j.language,
            include_constellations: j.include_constellations,
            constellation_boundaries: j.constellation_boundaries,
            equatorial_grid: j.equatorial_grid,
            horizontal_grid: j.horizontal_grid,
            grid_spacing_px: j.grid_spacing_px,
            viewport: j.viewport,
        }
    }
}

fn open_annotator(
    solver: &core::Solver,
    dso_path: *const c_char,
    names_path: *const c_char,
) -> Result<UnisolverAnnotator, String> {
    // Both assets may be NULL: without the DSO catalog there is no deep-sky layer, without the
    // names pack only English names; every other layer works (see `layers` in the result)
    let dso = if dso_path.is_null() {
        None
    } else {
        Some(unsafe { cstr(dso_path, "dso_path") }?)
    };
    let names = if names_path.is_null() {
        None
    } else {
        Some(unsafe { cstr(names_path, "names_path") }?)
    };
    Ok(UnisolverAnnotator {
        inner: solver.annotator(dso, names).map_err(|e| e.to_string())?,
    })
}

/// Builds an annotator from a single-database handle. `dso_path` / `names_path` may be NULL
/// (see the struct docs).
///
/// # Safety
/// `solver` is live; both paths are NULL or valid NUL-terminated strings; `error_out` as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_annotator_open(
    solver: *const UnisolverSolver,
    dso_path: *const c_char,
    names_path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut UnisolverAnnotator {
    clear_error(error_out);
    let run = || -> Result<UnisolverAnnotator, String> {
        let solver = solver.as_ref().ok_or("solver is NULL")?;
        open_annotator(&solver.solver, dso_path, names_path)
    };
    match run() {
        Ok(a) => Box::into_raw(Box::new(a)),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Builds an annotator from **one tier** of a pool. Pass the tier that solved the frame (the
/// `db` field of the solve JSON); NULL uses the widest tier. Narrow tiers have denser
/// catalogs, so annotate with the one that solved.
///
/// # Safety
/// `pool` is live; `db_name` is NULL or a valid NUL-terminated string; the rest as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_pool_annotator_open(
    pool: *const UnisolverPool,
    db_name: *const c_char,
    dso_path: *const c_char,
    names_path: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut UnisolverAnnotator {
    clear_error(error_out);
    let run = || -> Result<UnisolverAnnotator, String> {
        let pool = pool.as_ref().ok_or("pool is NULL")?;
        let tiers = pool.pool.tiers();
        let name = if db_name.is_null() {
            tiers
                .first()
                .map(|t| t.name.clone())
                .ok_or("pool is empty")?
        } else {
            cstr(db_name, "db_name")?.to_string()
        };
        let solver = pool
            .pool
            .solver(&name)
            .ok_or_else(|| format!("no such tier in pool: {name}"))?;
        open_annotator(solver, dso_path, names_path)
    };
    match run() {
        Ok(a) => Box::into_raw(Box::new(a)),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Languages in the names pack, as an OWNED JSON array. Empty means no names pack (English only).
///
/// # Safety
/// `annotator` is live; `error_out` as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_annotator_languages_json(
    annotator: *const UnisolverAnnotator,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let a = annotator.as_ref().ok_or("annotator is NULL")?;
        serde_json::to_string(&a.inner.languages()).map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Loads the constellation pack (`unisolver_constellations.bin`) into an annotator, for the
/// `include_constellations` and `constellation_boundaries` options. Returns true when loaded;
/// on failure returns false with `error_out` set, and the annotator stays usable (those layers
/// report themselves unavailable, with the reason, as a missing DSO catalog does).
///
/// # Safety
/// `annotator` is live and no other call is in flight on it; `path` is a valid NUL-terminated
/// string; `error_out` is NULL or a writable pointer slot.
#[no_mangle]
pub unsafe extern "C" fn unisolver_annotator_load_constellations(
    annotator: *mut UnisolverAnnotator,
    path: *const c_char,
    error_out: *mut *mut c_char,
) -> bool {
    clear_error(error_out);
    let run = || -> Result<(), String> {
        let a = annotator.as_mut().ok_or("annotator is NULL")?;
        a.inner
            .load_constellations(cstr(path, "path")?)
            .map_err(|e| e.to_string())
    };
    match run() {
        Ok(()) => true,
        Err(e) => {
            set_error(error_out, &e);
            false
        }
    }
}

/// Annotates a frame from the `wcs` object of the solve JSON, unchanged; returns OWNED annotation JSON.
///
/// `opts_json` may be NULL or `{}` (all defaults); fields are listed on `AnnotateOptsJson`.
/// The usual ones are `language` (`zh_cn` / `ja` / …), `observation_unix_ms` (required by the
/// solar-system and satellite layers), `observer` (moon parallax; required by satellites) and
/// `satellite_tle`.
///
/// The result is **data**: pixel coordinates (top-left origin), names, apparent sizes and
/// orientations for each layer, plus `layers` (availability and the reason for anything
/// unavailable or degraded). **Drawing is the caller's job**; this library does not render.
///
/// # Safety
/// `annotator` is live; `wcs_json` is a valid NUL-terminated string; `opts_json` is NULL or
/// a valid NUL-terminated string; `error_out` as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_annotate_json(
    annotator: *const UnisolverAnnotator,
    wcs_json: *const c_char,
    opts_json: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let a = annotator.as_ref().ok_or("annotator is NULL")?;
        let wcs: core::Wcs = serde_json::from_str(cstr(wcs_json, "wcs_json")?)
            .map_err(|e| format!("wcs_json: {e}"))?;
        let opts: core::AnnotateOptions = if opts_json.is_null() {
            AnnotateOptsJson::default().into()
        } else {
            serde_json::from_str::<AnnotateOptsJson>(cstr(opts_json, "opts_json")?)
                .map_err(|e| format!("opts_json: {e}"))?
                .into()
        };
        serde_json::to_string(&a.inner.annotate(&wcs, &opts)).map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Shared shape of the batch transforms: parse the WCS, map `n` points of `input` into `out`.
unsafe fn wcs_batch(
    wcs_json: *const c_char,
    input: *const f64,
    n: usize,
    out: *mut f64,
    map: impl Fn(&core::Wcs, &[[f64; 2]]) -> Vec<Option<[f64; 2]>>,
) -> Result<(), String> {
    let wcs: core::Wcs =
        serde_json::from_str(cstr(wcs_json, "wcs_json")?).map_err(|e| format!("wcs_json: {e}"))?;
    if n == 0 {
        return Ok(());
    }
    if input.is_null() || out.is_null() {
        return Err("input and out must not be NULL".into());
    }
    let pts: Vec<[f64; 2]> = std::slice::from_raw_parts(input, 2 * n)
        .as_chunks::<2>()
        .0
        .to_vec();
    let res = map(&wcs, &pts);
    let out = std::slice::from_raw_parts_mut(out, 2 * n);
    for (i, r) in res.into_iter().enumerate() {
        let [a, b] = r.unwrap_or([f64::NAN, f64::NAN]);
        (out[2 * i], out[2 * i + 1]) = (a, b);
    }
    Ok(())
}

/// Batch sky → pixel, for drawing your own overlays: `input` holds `n` pairs `ra, dec`
/// (degrees), `out` receives `n` pairs `x, y` (top-left pixels). A pair the lens model cannot
/// place (behind the camera, or beyond 1.2× the frame's corner distance, where the distortion
/// polynomial folds points back in) comes back as NaN, NaN. `wcs_json` is the solve JSON's
/// `wcs`, unchanged. Returns false with `error_out` set on bad arguments.
///
/// # Safety
/// `wcs_json` is a valid NUL-terminated string; `input` and `out` point to `2 * n` doubles
/// (they may be the same buffer); `error_out` is NULL or a writable pointer slot.
#[no_mangle]
pub unsafe extern "C" fn unisolver_wcs_sky_to_pixels(
    wcs_json: *const c_char,
    input: *const f64,
    n: usize,
    out: *mut f64,
    error_out: *mut *mut c_char,
) -> bool {
    clear_error(error_out);
    match wcs_batch(wcs_json, input, n, out, |w, p| w.sky_to_pixels(p)) {
        Ok(()) => true,
        Err(e) => {
            set_error(error_out, &e);
            false
        }
    }
}

/// Batch pixel → sky: `n` pairs `x, y` in, `n` pairs `ra, dec` (degrees) out; NaN, NaN for
/// pixels more than a quarter frame outside the image. Otherwise as
/// [`unisolver_wcs_sky_to_pixels`].
///
/// # Safety
/// As [`unisolver_wcs_sky_to_pixels`].
#[no_mangle]
pub unsafe extern "C" fn unisolver_wcs_pixels_to_sky(
    wcs_json: *const c_char,
    input: *const f64,
    n: usize,
    out: *mut f64,
    error_out: *mut *mut c_char,
) -> bool {
    clear_error(error_out);
    match wcs_batch(wcs_json, input, n, out, |w, p| w.pixels_to_sky(p)) {
        Ok(()) => true,
        Err(e) => {
            set_error(error_out, &e);
            false
        }
    }
}

/// Releases an annotator. Accepts NULL.
///
/// # Safety
/// `annotator` must come from this library and not be freed yet; no call may be in flight on it.
#[no_mangle]
pub unsafe extern "C" fn unisolver_annotator_close(annotator: *mut UnisolverAnnotator) {
    if !annotator.is_null() {
        drop(Box::from_raw(annotator));
    }
}

/// Opaque calibration session handle. Release with [`unisolver_calibration_close`].
///
/// **Not thread-safe**: `add_image_json` changes the session, so no other call may be in
/// flight on the same session (separate sessions are independent).
pub struct UnisolverCalibration {
    inner: core::CalibrationSession,
}

/// Opens a calibration session: feed it several images of the **same size**, then fit the
/// camera (including distortion). On real data the radial model reduced the RMSE from
/// 11.3 px to 1.4 px.
///
/// # Safety
/// `solver` is live; `error_out` as above.
#[no_mangle]
pub unsafe extern "C" fn unisolver_calibration_open(
    solver: *const UnisolverSolver,
    error_out: *mut *mut c_char,
) -> *mut UnisolverCalibration {
    clear_error(error_out);
    let run = || -> Result<UnisolverCalibration, String> {
        let solver = solver.as_ref().ok_or("solver is NULL")?;
        Ok(UnisolverCalibration {
            inner: solver
                .solver
                .new_calibration_session()
                .map_err(|e| e.to_string())?,
        })
    };
    match run() {
        Ok(c) => Box::into_raw(Box::new(c)),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Adds an image to the session and returns its solve JSON (same shape as the solve entries).
/// **Only solved frames join the session**; for the others the status tells the caller to
/// ask for another shot. `opts_json` as [`unisolver_solve_image_json_opts`] (NULL = defaults).
///
/// # Safety
/// `cal` is live and **no other call is in flight**; `path` is a valid NUL-terminated string;
/// `opts_json` is NULL or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn unisolver_calibration_add_image_json(
    cal: *mut UnisolverCalibration,
    path: *const c_char,
    opts_json: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let cal = cal.as_mut().ok_or("calibration is NULL")?;
        let path = cstr(path, "path")?;
        let (frame, _meta) = core::imageio::load_image(path).map_err(|e| e.to_string())?;
        let (mut base, _known) = SolveOptsJson::parse(opts_json)?.into_core()?;
        if let Some(c) = base.camera.clone() {
            base.fov_estimate_deg = c.horizontal_fov_deg(frame.width) as f32;
            base.fov_max_error_deg = None;
        }
        let fov = base.fov_estimate_deg;
        let out = cal
            .inner
            .add_frame(&frame, &base)
            .map_err(|e| e.to_string())?;
        let a = vec![AttemptJson {
            fov_deg: fov,
            status: format!("{:?}", out.status),
            solve_ms: out.timing.solve_ms,
            db: None,
        }];
        serde_json::to_string(&build_solve_json(out, a, None)).map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Frames accepted into the session (solved); -1 when `cal` is NULL.
///
/// # Safety
/// `cal` is NULL or live.
#[no_mangle]
pub unsafe extern "C" fn unisolver_calibration_count(cal: *const UnisolverCalibration) -> i32 {
    match cal.as_ref() {
        Some(c) => c.inner.count() as i32,
        None => -1,
    }
}

/// Fits the camera. `model_json` is NULL or `{"model":"radial"}` / `{"model":"polynomial","order":3}`.
/// Returns an OWNED report: `camera` (persist it and pass it back as `camera` to skip the
/// ladder), `rmse_before_px` / `rmse_after_px` / `n_inliers` / `n_outliers` / `frames_used`.
///
/// # Safety
/// `cal` is live; `model_json` is NULL or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn unisolver_calibration_fit_json(
    cal: *const UnisolverCalibration,
    model_json: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    clear_error(error_out);
    let run = || -> Result<String, String> {
        let cal = cal.as_ref().ok_or("calibration is NULL")?;
        #[derive(serde::Deserialize)]
        #[serde(default)]
        struct ModelJson {
            model: String,
            order: u8,
        }
        impl Default for ModelJson {
            fn default() -> Self {
                Self {
                    model: "radial".into(),
                    order: 3,
                }
            }
        }
        let m = if model_json.is_null() {
            ModelJson::default()
        } else {
            let t = cstr(model_json, "model_json")?;
            if t.trim().is_empty() {
                ModelJson::default()
            } else {
                serde_json::from_str(t).map_err(|e| format!("model_json: {e}"))?
            }
        };
        let model = match m.model.as_str() {
            "radial" => core::CalibModel::Radial,
            "polynomial" => core::CalibModel::Polynomial { order: m.order },
            other => return Err(format!("unknown model '{other}' (radial|polynomial)")),
        };
        let rep = cal.inner.fit(model).map_err(|e| e.to_string())?;
        serde_json::to_string(&rep).map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => to_owned_cstring(s),
        Err(e) => {
            set_error(error_out, &e);
            ptr::null_mut()
        }
    }
}

/// Releases a calibration session. Accepts NULL.
///
/// # Safety
/// `cal` must come from this library and not be freed yet; no call may be in flight on it.
#[no_mangle]
pub unsafe extern "C" fn unisolver_calibration_close(cal: *mut UnisolverCalibration) {
    if !cal.is_null() {
        drop(Box::from_raw(cal));
    }
}

/// Releases a string returned by this library. Accepts NULL.
///
/// # Safety
/// `value` must be a string returned by this library and not freed yet.
#[no_mangle]
pub unsafe extern "C" fn unisolver_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn version_is_static_nul_terminated() {
        let v = unsafe { CStr::from_ptr(unisolver_version()) };
        assert!(v.to_str().unwrap().starts_with("unisolver "));
    }

    #[test]
    fn open_solve_close_roundtrip_via_c_surface() {
        // Reuse the synth test database file (same name as core's integration tests)
        let db = CString::new(unisolver_synth::test_db_file("unisolver_core_test.db")).unwrap();
        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = unsafe { unisolver_open(db.as_ptr(), &mut err) };
        assert!(!solver.is_null());
        assert!(err.is_null());

        // Render a synthetic frame to a temporary 16-bit PNG and solve it through the C surface
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
        let png_path = std::env::temp_dir().join("cabi_test.png");
        let maxv = img.iter().cloned().fold(0.0f32, f32::max).max(1.0);
        let buf: Vec<u16> = img.iter().map(|v| (v / maxv * 65535.0) as u16).collect();
        image::save_buffer(
            &png_path,
            &buf.iter()
                .flat_map(|v| v.to_ne_bytes())
                .collect::<Vec<u8>>(),
            1024,
            768,
            image::ExtendedColorType::L16,
        )
        .unwrap();

        let cp = CString::new(png_path.to_str().unwrap()).unwrap();
        let out = unsafe { unisolver_solve_image_json(solver, cp.as_ptr(), &mut err) };
        assert!(!out.is_null(), "err: {:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        assert!(json.contains("\"status\":\"Ok\""), "{json}");
        assert!(json.contains("\"ra_deg\":12"), "{json}"); // ≈120.x
        unsafe { unisolver_string_free(out) };

        // NULL tolerance
        unsafe {
            unisolver_string_free(std::ptr::null_mut());
            unisolver_close(solver);
            unisolver_close(std::ptr::null_mut());
        }
    }

    #[test]
    fn pool_open_register_solve_close_via_c_surface() {
        // Two independent synthetic databases: wrong routing really fails
        let dir = std::env::temp_dir().join("cabi_pool_test");
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
        let cdir = CString::new(dir.to_str().unwrap()).unwrap();
        let mut err: *mut c_char = std::ptr::null_mut();
        let pool = unsafe { unisolver_pool_open(cdir.as_ptr(), &mut err) };
        assert!(!pool.is_null() && err.is_null());

        let tiers = unsafe { unisolver_pool_tiers_json(pool, &mut err) };
        assert!(!tiers.is_null());
        let tj = unsafe { CStr::from_ptr(tiers) }
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            tj.contains("unisolver_15_40") && tj.contains("unisolver_8_15"),
            "{tj}"
        );
        assert!(tj.contains("\"skipped\":[]"), "{tj}");
        unsafe { unisolver_string_free(tiers) };

        // Registering again is idempotent (install-then-register flows report a file twice)
        let again = CString::new(dir.join("unisolver_8_15.db").to_str().unwrap()).unwrap();
        let reg = unsafe { unisolver_pool_register(pool, again.as_ptr(), &mut err) };
        assert!(!reg.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        unsafe { unisolver_string_free(reg) };
        let tiers = unsafe { unisolver_pool_tiers_json(pool, &mut err) };
        let tj = unsafe { CStr::from_ptr(tiers) }
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(tj.matches("\"name\"").count(), 2, "{tj}");
        unsafe { unisolver_string_free(tiers) };

        // A 10° frame rendered from the narrow catalog: with no header hint, the pool must sweep to the narrow tier
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
        let png = std::env::temp_dir().join("cabi_pool_test.png");
        let maxv = img.iter().cloned().fold(0.0f32, f32::max).max(1.0);
        let buf: Vec<u8> = img
            .iter()
            .flat_map(|v| ((v / maxv * 65535.0) as u16).to_ne_bytes())
            .collect();
        image::save_buffer(&png, &buf, 1024, 768, image::ExtendedColorType::L16).unwrap();

        let cp = CString::new(png.to_str().unwrap()).unwrap();
        let out = unsafe { unisolver_pool_solve_image_json(pool, cp.as_ptr(), &mut err) };
        assert!(!out.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        assert!(json.contains("\"status\":\"Ok\""), "{json}");
        assert!(json.contains("\"db\":\"unisolver_8_15\""), "{json}");
        unsafe { unisolver_string_free(out) };

        unsafe {
            unisolver_pool_close(pool);
            unisolver_pool_close(std::ptr::null_mut());
        }
    }

    #[test]
    fn pool_open_on_an_empty_dir_is_an_error() {
        let dir = std::env::temp_dir().join("cabi_pool_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let cdir = CString::new(dir.to_str().unwrap()).unwrap();
        let mut err: *mut c_char = std::ptr::null_mut();
        let pool = unsafe { unisolver_pool_open(cdir.as_ptr(), &mut err) };
        assert!(pool.is_null() && !err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_string_lossy().to_string();
        assert!(msg.contains("no usable database"), "{msg}");
        unsafe { unisolver_string_free(err) };
    }

    #[test]
    fn satellites_json_via_c_surface() {
        // Historical ISS TLE (epoch 2024-001.5), geometry-magnitude check
        let tle = CString::new(
            "ISS (ZARYA)\n\
             1 25544U 98067A   24001.50000000  .00016717  00000-0  30777-3 0  9991\n\
             2 25544  51.6400 208.9163 0006317  69.9862 290.2117 15.49560538429085",
        )
        .unwrap();
        let mut err: *mut c_char = std::ptr::null_mut();
        let out = unsafe {
            unisolver_satellites_json(tle.as_ptr(), 1_704_110_400_000, 31.2, 121.5, 10.0, &mut err)
        };
        assert!(!out.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        assert!(json.contains("\"name\":\"ISS (ZARYA)\""), "{json}");
        assert!(json.contains("\"range_km\""), "{json}");
        unsafe { unisolver_string_free(out) };

        // An out-of-range site and a garbage TLE are errors (not empty arrays)
        for (lat, text) in [(120.0, "x"), (31.2, "not a tle")] {
            let t = CString::new(text).unwrap();
            let r = unsafe { unisolver_satellites_json(t.as_ptr(), 0, lat, 121.5, 10.0, &mut err) };
            assert!(r.is_null() && !err.is_null(), "should have failed: {text}");
            unsafe { unisolver_string_free(err) };
        }
    }

    /// DSO outlines cross the C surface and `dso_outlines: false` turns them off.
    #[test]
    fn annotate_outlines_via_c_surface() {
        let db = CString::new(unisolver_synth::test_db_file("unisolver_core_test.db")).unwrap();
        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = unsafe { unisolver_open(db.as_ptr(), &mut err) };
        assert!(!solver.is_null());
        let dso = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/unisolver_flutter/assets/unisolver_dso.bin");
        let dso_c = CString::new(dso.to_str().unwrap()).unwrap();
        let ann =
            unsafe { unisolver_annotator_open(solver, dso_c.as_ptr(), std::ptr::null(), &mut err) };
        assert!(!ann.is_null());

        // A 20° field on M42, which the bundled catalog outlines
        let cam = core::CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
        let wcs = core::Wcs {
            width: 1024,
            height: 768,
            cd: [[0.0; 2]; 2],
            crval_deg: [83.82, -5.39],
            theta_rad: 0.0,
            camera: cam,
        };
        let wcs_json = CString::new(serde_json::to_string(&wcs).unwrap()).unwrap();
        let annotate = |opts: &str| {
            let opts = CString::new(opts).unwrap();
            let mut err: *mut c_char = std::ptr::null_mut();
            let out =
                unsafe { unisolver_annotate_json(ann, wcs_json.as_ptr(), opts.as_ptr(), &mut err) };
            assert!(!out.is_null());
            let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
            unsafe { unisolver_string_free(out) };
            json
        };
        assert!(annotate("{}").contains("\"contours\""));
        assert!(!annotate(r#"{"dso_outlines":false}"#).contains("\"contours\""));

        unsafe {
            unisolver_annotator_close(ann);
            unisolver_close(solver);
        }
    }

    /// The annotation surface end to end: build → languages → annotate with the solve's wcs as-is → release.
    /// Constellations through the C surface: load the pack, ask for the layers, get figures
    /// and boundaries back; a bad path is an error and leaves the annotator usable.
    #[test]
    fn constellations_via_c_surface() {
        use unisolver_core::constellations::{
            BoundaryEdge, ConstellationFigure, ConstellationPack,
        };
        let dir = std::env::temp_dir().join(format!("cabi_ucon_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pack_path = dir.join("unisolver_constellations.bin");
        ConstellationPack {
            constellations: vec![
                ConstellationFigure {
                    abbr: "Ori".into(),
                    name: "Orion".into(),
                    lines: vec![vec![[88.79, 7.41], [81.28, 6.35], [78.63, -8.20]]],
                    label: [83.0, 1.0],
                },
                ConstellationFigure {
                    abbr: "Tau".into(),
                    name: "Taurus".into(),
                    lines: vec![vec![[68.98, 16.51], [84.41, 21.14]]],
                    label: [70.0, 18.0],
                },
            ],
            boundaries: vec![BoundaryEdge {
                between: [0, 1],
                points: (0..=12).map(|k| [86.0, 12.0 - k as f32]).collect(),
            }],
        }
        .write(pack_path.to_str().unwrap())
        .unwrap();

        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = open_test_solver(&mut err);
        let ann = unsafe {
            unisolver_annotator_open(solver, std::ptr::null(), std::ptr::null(), &mut err)
        };
        let bad = CString::new("/nonexistent/ucon.bin").unwrap();
        assert!(!unsafe { unisolver_annotator_load_constellations(ann, bad.as_ptr(), &mut err) });
        assert!(!err.is_null());
        unsafe { unisolver_string_free(err) };
        err = std::ptr::null_mut();
        let good = CString::new(pack_path.to_str().unwrap()).unwrap();
        assert!(unsafe { unisolver_annotator_load_constellations(ann, good.as_ptr(), &mut err) });

        let wcs = serde_json::to_string(&core::Wcs {
            width: 1024,
            height: 768,
            cd: [[0.0; 2]; 2],
            crval_deg: [83.8, -1.0],
            theta_rad: 0.0,
            camera: core::CameraParams::from_horizontal_fov(30.0, 1024, 768).unwrap(),
        })
        .unwrap();
        let wcs = CString::new(wcs).unwrap();
        let opts =
            CString::new(r#"{"include_constellations":true,"constellation_boundaries":true}"#)
                .unwrap();
        let out = unsafe { unisolver_annotate_json(ann, wcs.as_ptr(), opts.as_ptr(), &mut err) };
        assert!(!out.is_null());
        let v: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
        unsafe { unisolver_string_free(out) };
        assert_eq!(v["layers"]["constellations"], true, "{v}");
        let names: Vec<&str> = v["constellations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"Orion"), "{names:?}");
        assert_eq!(
            v["boundaries"][0]["between"],
            serde_json::json!(["Ori", "Tau"])
        );
        // Not asked for: no constellation keys filled
        let out = unsafe { unisolver_annotate_json(ann, wcs.as_ptr(), std::ptr::null(), &mut err) };
        let v: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
        unsafe { unisolver_string_free(out) };
        assert_eq!(v["constellations"], serde_json::json!([]));
        unsafe {
            unisolver_annotator_close(ann);
            unisolver_close(solver);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Grids and transforms through the C surface: a zoomed viewport gets a finer grid,
    /// labelled on its edges; the batch transforms round-trip and mark the far side NaN.
    #[test]
    fn grid_and_transforms_via_c_surface() {
        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = open_test_solver(&mut err);
        let ann = unsafe {
            unisolver_annotator_open(solver, std::ptr::null(), std::ptr::null(), &mut err)
        };
        let wcs = serde_json::to_string(&core::Wcs {
            width: 1920,
            height: 1080,
            cd: [[0.0; 2]; 2],
            crval_deg: [83.8, 0.0],
            theta_rad: 0.0,
            camera: core::CameraParams::from_horizontal_fov(70.0, 1920, 1080).unwrap(),
        })
        .unwrap();
        let wcs = CString::new(wcs).unwrap();
        let annotate = |opts: &str| -> serde_json::Value {
            let o = CString::new(opts).unwrap();
            let mut e: *mut c_char = std::ptr::null_mut();
            let out = unsafe { unisolver_annotate_json(ann, wcs.as_ptr(), o.as_ptr(), &mut e) };
            assert!(!out.is_null());
            let v = serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
            unsafe { unisolver_string_free(out) };
            v
        };
        let whole = annotate(r#"{"equatorial_grid":true}"#);
        assert_eq!(whole["layers"]["grid"], true);
        let steps = |v: &serde_json::Value| -> f64 {
            let mut decs: Vec<f64> = v["grid"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|g| g["kind"] == "Dec")
                .map(|g| g["value_deg"].as_f64().unwrap())
                .collect();
            decs.sort_by(f64::total_cmp);
            decs.windows(2)
                .map(|w| w[1] - w[0])
                .fold(f64::MAX, f64::min)
        };
        let zoomed = annotate(
            r#"{"equatorial_grid":true,"viewport":{"x":800,"y":450,"width":320,"height":180,"scale":6}}"#,
        );
        assert!(
            steps(&zoomed) < steps(&whole),
            "{} vs {}",
            steps(&zoomed),
            steps(&whole)
        );
        let label = &zoomed["grid"][0]["label"];
        assert!(
            ["Left", "Right", "Top", "Bottom"].contains(&label["edge"].as_str().unwrap()),
            "{label}"
        );

        let input = [83.8, 0.0, 263.8, 0.0];
        let mut px = [0.0f64; 4];
        assert!(unsafe {
            unisolver_wcs_sky_to_pixels(wcs.as_ptr(), input.as_ptr(), 2, px.as_mut_ptr(), &mut err)
        });
        assert!(
            (px[0] - 959.5).abs() < 1e-6 && (px[1] - 539.5).abs() < 1e-6,
            "{px:?}"
        );
        assert!(px[2].is_nan() && px[3].is_nan());
        let mut back = [0.0f64; 2];
        assert!(unsafe {
            unisolver_wcs_pixels_to_sky(wcs.as_ptr(), px.as_ptr(), 1, back.as_mut_ptr(), &mut err)
        });
        assert!((back[0] - 83.8).abs() < 1e-9 && back[1].abs() < 1e-9);
        assert!(!unsafe {
            unisolver_wcs_sky_to_pixels(
                wcs.as_ptr(),
                std::ptr::null(),
                1,
                px.as_mut_ptr(),
                &mut err,
            )
        });
        unsafe {
            unisolver_string_free(err);
            unisolver_annotator_close(ann);
            unisolver_close(solver);
        }
    }

    #[test]
    fn annotate_json_via_c_surface() {
        let db = CString::new(unisolver_synth::test_db_file("unisolver_core_test.db")).unwrap();
        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = unsafe { unisolver_open(db.as_ptr(), &mut err) };
        assert!(!solver.is_null());

        // Use the real names pack when present (without it nothing should fail, names just stay English)
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let names = root.join("packages/unisolver_flutter/lib/optional/unisolver_names.bin");
        let names_c = names
            .exists()
            .then(|| CString::new(names.to_str().unwrap()).unwrap());
        let names_ptr = names_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        let ann =
            unsafe { unisolver_annotator_open(solver, std::ptr::null(), names_ptr, &mut err) };
        assert!(!ann.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });

        let langs = unsafe { unisolver_annotator_languages_json(ann, &mut err) };
        assert!(!langs.is_null());
        let langs_s = unsafe { CStr::from_ptr(langs) }
            .to_str()
            .unwrap()
            .to_string();
        if names.exists() {
            assert!(langs_s.contains("zh_cn"), "{langs_s}");
        }
        unsafe { unisolver_string_free(langs) };

        // The field points at Vega; the wcs is passed in core::Wcs's serialized form (= the solve JSON object)
        let cam = core::CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
        let wcs = core::Wcs {
            width: 1024,
            height: 768,
            cd: [[0.0; 2]; 2],
            crval_deg: [279.2347, 38.7837],
            theta_rad: 0.0,
            camera: cam,
        };
        let wcs_json = CString::new(serde_json::to_string(&wcs).unwrap()).unwrap();

        // NULL opts = all defaults (English)
        let out =
            unsafe { unisolver_annotate_json(ann, wcs_json.as_ptr(), std::ptr::null(), &mut err) };
        assert!(!out.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        assert!(json.contains("\"named_stars\""), "{json}");
        assert!(json.contains("Vega"), "{json}");
        assert!(json.contains("\"layers\""), "{json}");
        unsafe { unisolver_string_free(out) };

        // Partial opts must parse too (missing fields = defaults)
        let opts = CString::new(r#"{"language":"zh_cn","max_stars":10}"#).unwrap();
        let out =
            unsafe { unisolver_annotate_json(ann, wcs_json.as_ptr(), opts.as_ptr(), &mut err) };
        assert!(!out.is_null());
        let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        if names.exists() {
            assert!(json.contains("织女一"), "{json}");
        }
        unsafe { unisolver_string_free(out) };

        // A bad wcs or bad opts is an error, not a crash
        let bad = CString::new("{not json").unwrap();
        let r = unsafe { unisolver_annotate_json(ann, bad.as_ptr(), std::ptr::null(), &mut err) };
        assert!(r.is_null() && !err.is_null());
        unsafe { unisolver_string_free(err) };

        unsafe {
            unisolver_annotator_close(ann);
            unisolver_annotator_close(std::ptr::null_mut());
            unisolver_close(solver);
        }
    }

    /// Saves a synthetic star field as a 16-bit PNG and returns the path
    fn render_png(name: &str, ra: f64, dec: f64, fov: f32) -> std::path::PathBuf {
        let q = unisolver_synth::look_at(ra, dec, 15.0);
        let img = unisolver_synth::render(
            unisolver_synth::test_db().star_catalog.stars(),
            &q,
            fov,
            1024,
            768,
            &unisolver_synth::RenderParams::default(),
            5,
        );
        let p = std::env::temp_dir().join(name);
        let maxv = img.iter().cloned().fold(0.0f32, f32::max).max(1.0);
        let buf: Vec<u8> = img
            .iter()
            .flat_map(|v| ((v / maxv * 65535.0) as u16).to_ne_bytes())
            .collect();
        image::save_buffer(&p, &buf, 1024, 768, image::ExtendedColorType::L16).unwrap();
        p
    }

    fn open_test_solver(err: &mut *mut c_char) -> *mut UnisolverSolver {
        let db = CString::new(unisolver_synth::test_db_file("unisolver_core_test.db")).unwrap();
        unsafe { unisolver_open(db.as_ptr(), err) }
    }

    /// Solve with options: a known FOV tries one rung; a tracking hint takes the fast path; bad options are errors.
    #[test]
    fn solve_with_opts_via_c_surface() {
        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = open_test_solver(&mut err);
        assert!(!solver.is_null());
        let png = render_png("cabi_opts.png", 120.0, 40.0, 20.0);
        let path = CString::new(png.to_str().unwrap()).unwrap();

        // (1) Known FOV → **one attempt** (no ladder)
        let opts = CString::new(r#"{"fov_deg":20.0,"profile":"clean"}"#).unwrap();
        let out = unsafe {
            unisolver_solve_image_json_opts(solver, path.as_ptr(), opts.as_ptr(), &mut err)
        };
        assert!(!out.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        assert!(json.contains("\"status\":\"Ok\""), "{json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["attempts"].as_array().unwrap().len(), 1, "{json}");
        let q = v["solution"]["wcs"].clone();
        assert!(!q.is_null());
        unsafe { unisolver_string_free(out) };

        // (2) NULL / {} mean the same as the old entry
        for o in [std::ptr::null(), c"{}".as_ptr()] {
            let r = unsafe { unisolver_solve_image_json_opts(solver, path.as_ptr(), o, &mut err) };
            assert!(!r.is_null());
            unsafe { unisolver_string_free(r) };
        }

        // (3) Tracking: use the previous attitude as the hint (quaternion first)
        let full = unsafe {
            unisolver_solve_image_json_opts(solver, path.as_ptr(), opts.as_ptr(), &mut err)
        };
        let v: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(full) }.to_str().unwrap()).unwrap();
        unsafe { unisolver_string_free(full) };
        // Rebuild an attitude hint from the solved ra/dec; the synth truth is simpler here
        let quat = unisolver_core::test_support::wxyz(&unisolver_synth::look_at(120.0, 40.0, 15.0));
        assert!((v["solution"]["ra_deg"].as_f64().unwrap() - 120.0).abs() < 0.2);
        let track = CString::new(format!(
            r#"{{"fov_deg":20.0,"profile":"clean","attitude_hint_wxyz":[{},{},{},{}]}}"#,
            quat[0], quat[1], quat[2], quat[3]
        ))
        .unwrap();
        let out = unsafe {
            unisolver_solve_image_json_opts(solver, path.as_ptr(), track.as_ptr(), &mut err)
        };
        assert!(!out.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        assert!(json.contains("\"status\":\"Ok\""), "tracking: {json}");
        unsafe { unisolver_string_free(out) };

        // (4) Tracking without a scale → an error (not a silent blind solve)
        let bad = CString::new(r#"{"attitude_hint_wxyz":[1.0,0.0,0.0,0.0]}"#).unwrap();
        let r = unsafe {
            unisolver_solve_image_json_opts(solver, path.as_ptr(), bad.as_ptr(), &mut err)
        };
        assert!(r.is_null() && !err.is_null());
        let msg = unsafe { CStr::from_ptr(err) }.to_string_lossy().to_string();
        assert!(msg.contains("needs fov_deg or camera"), "{msg}");
        unsafe { unisolver_string_free(err) };

        // (5) An unknown profile name is an error
        let bad = CString::new(r#"{"profile":"nope"}"#).unwrap();
        let r = unsafe {
            unisolver_solve_image_json_opts(solver, path.as_ptr(), bad.as_ptr(), &mut err)
        };
        assert!(r.is_null() && !err.is_null());
        unsafe { unisolver_string_free(err) };

        unsafe { unisolver_close(solver) };
    }

    /// Calibration session: frames in → fit → the camera feeds back into the solve entries.
    #[test]
    fn calibration_session_via_c_surface() {
        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = open_test_solver(&mut err);
        let cal = unsafe { unisolver_calibration_open(solver, &mut err) };
        assert!(!cal.is_null());
        assert_eq!(unsafe { unisolver_calibration_count(cal) }, 0);
        assert_eq!(unsafe { unisolver_calibration_count(std::ptr::null()) }, -1);

        let opts = CString::new(r#"{"fov_deg":20.0,"profile":"clean"}"#).unwrap();
        for (i, (ra, dec)) in [(120.0, 40.0), (123.0, 41.0), (118.0, 38.5)]
            .iter()
            .enumerate()
        {
            let png = render_png(&format!("cabi_calib_{i}.png"), *ra, *dec, 20.0);
            let path = CString::new(png.to_str().unwrap()).unwrap();
            let out = unsafe {
                unisolver_calibration_add_image_json(cal, path.as_ptr(), opts.as_ptr(), &mut err)
            };
            assert!(!out.is_null(), "{:?}", unsafe {
                err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
            });
            unsafe { unisolver_string_free(out) };
        }
        assert!(unsafe { unisolver_calibration_count(cal) } >= 2);

        let out = unsafe { unisolver_calibration_fit_json(cal, std::ptr::null(), &mut err) };
        assert!(!out.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        let json = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        let rep: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            rep["camera"]["focal_length_px"].as_f64().unwrap() > 100.0,
            "{json}"
        );
        assert!(rep["rmse_after_px"].as_f64().unwrap() <= rep["rmse_before_px"].as_f64().unwrap());
        unsafe { unisolver_string_free(out) };

        // Feed the fitted camera back into a solve: the known-camera path, one attempt
        let camera = rep["camera"].to_string();
        let png = render_png("cabi_calib_use.png", 200.0, -30.0, 20.0);
        let path = CString::new(png.to_str().unwrap()).unwrap();
        let opts = CString::new(format!(r#"{{"camera":{camera},"profile":"clean"}}"#)).unwrap();
        let out = unsafe {
            unisolver_solve_image_json_opts(solver, path.as_ptr(), opts.as_ptr(), &mut err)
        };
        assert!(!out.is_null(), "{:?}", unsafe {
            err.as_ref().map(|_| CStr::from_ptr(err).to_string_lossy())
        });
        let v: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
        assert_eq!(v["attempts"].as_array().unwrap().len(), 1);
        assert_eq!(v["status"], "Ok");
        unsafe { unisolver_string_free(out) };

        // An unknown model name is an error; a NULL cal does not crash
        let bad = CString::new(r#"{"model":"nope"}"#).unwrap();
        let r = unsafe { unisolver_calibration_fit_json(cal, bad.as_ptr(), &mut err) };
        assert!(r.is_null() && !err.is_null());
        unsafe { unisolver_string_free(err) };
        unsafe {
            unisolver_calibration_close(cal);
            unisolver_calibration_close(std::ptr::null_mut());
            unisolver_close(solver);
        }
    }

    /// Direct frames: no disk, four pixel kinds, padded stride, tracking hint, bad parameters are errors.
    /// EXIF through the C surface: a JPEG's 35 mm focal length puts its rung first and its
    /// capture time comes back in the JSON; a decoded frame (the HEIC path) gets the same from
    /// `focal_length_35mm` and `observation_unix_ms` in the options.
    #[test]
    fn exif_hint_and_time_via_c_surface() {
        use image::ImageEncoder;
        use unisolver_synth::exif::{jpeg_with_exif, ExifFields};
        // 65 mm equivalent on 4:3 = 29.8° horizontal, inside the 15–40° test tier
        let (w, h, focal) = (1024u32, 768u32, 65u16);
        // CIPA: tan(diagonal/2) = 21.633 / focal; the horizontal share follows the aspect
        let fov = 2.0
            * (21.633 / focal as f64 * w as f64 / ((w * w + h * h) as f64).sqrt())
                .atan()
                .to_degrees();
        let q = unisolver_synth::look_at(200.0, 25.0, 30.0);
        let img = unisolver_synth::render(
            unisolver_synth::test_db().star_catalog.stars(),
            &q,
            fov as f32,
            w,
            h,
            &unisolver_synth::RenderParams::default(),
            11,
        );
        let maxv = img.iter().cloned().fold(0.0f32, f32::max).max(1.0);
        let px: Vec<u8> = img.iter().map(|v| (v / maxv * 255.0) as u8).collect();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .write_image(&px, w, h, image::ExtendedColorType::L8)
            .unwrap();
        let exif = ExifFields {
            focal_35mm: Some(focal),
            date_time_original: Some("2026:03:19 08:03:09".into()),
            offset_time_original: Some("+08:00".into()),
            gps_position: Some((
                "N".into(),
                [(31, 1), (12, 1), (0, 1)],
                "E".into(),
                [(121, 1), (30, 1), (0, 1)],
                false,
                (10, 1),
            )),
            ..Default::default()
        };
        let path = std::env::temp_dir().join(format!("cabi_exif_{}.jpg", std::process::id()));
        std::fs::write(&path, jpeg_with_exif(&jpeg, &exif)).unwrap();

        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = open_test_solver(&mut err);
        let opts = CString::new(r#"{"profile":"clean"}"#).unwrap();
        let cp = CString::new(path.to_str().unwrap()).unwrap();
        let out = unsafe {
            unisolver_solve_image_json_opts(solver, cp.as_ptr(), opts.as_ptr(), &mut err)
        };
        assert!(!out.is_null());
        let v: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
        unsafe { unisolver_string_free(out) };
        assert_eq!(v["status"], "Ok", "{v}");
        assert!(
            (v["attempts"][0]["fov_deg"].as_f64().unwrap() - fov).abs() < 0.05,
            "{v}"
        );
        // 2026-03-19T00:03:09Z
        assert_eq!(v["observation_unix_ms"], 1_773_878_589_000i64);
        assert_eq!(v["observer"]["lon_deg"], 121.5, "{v}");
        assert_eq!(v["observer"]["alt_m"], 10.0, "{v}");

        // The frame entry of a one-tier pool, EXIF given by the caller
        let dir = std::env::temp_dir().join(format!("cabi_exif_pool_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(
            unisolver_synth::test_db_file("unisolver_core_test.db"),
            dir.join("unisolver_15_40.db"),
        )
        .unwrap();
        let cdir = CString::new(dir.to_str().unwrap()).unwrap();
        let pool = unsafe { unisolver_pool_open(cdir.as_ptr(), &mut err) };
        assert!(!pool.is_null());
        let kind = CString::new("luma8").unwrap();
        let opts = CString::new(
            r#"{"profile":"clean","focal_length_35mm":65,"observation_unix_ms":1773878589000}"#,
        )
        .unwrap();
        let out = unsafe {
            unisolver_pool_solve_frame_json_opts(
                pool,
                px.as_ptr(),
                px.len(),
                w,
                h,
                kind.as_ptr(),
                0,
                opts.as_ptr(),
                &mut err,
            )
        };
        assert!(!out.is_null());
        let v: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(out) }.to_str().unwrap()).unwrap();
        unsafe { unisolver_string_free(out) };
        assert_eq!(v["status"], "Ok", "{v}");
        assert_eq!(v["attempts"].as_array().unwrap().len(), 1, "{v}");
        assert!(
            (v["attempts"][0]["fov_deg"].as_f64().unwrap() - fov).abs() < 0.05,
            "{v}"
        );
        assert_eq!(v["observation_unix_ms"], 1_773_878_589_000i64);
        unsafe {
            unisolver_pool_close(pool);
            unisolver_close(solver);
        }
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn solve_frame_buffer_via_c_surface() {
        let mut err: *mut c_char = std::ptr::null_mut();
        let solver = open_test_solver(&mut err);
        assert!(!solver.is_null());

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
        let opts = CString::new(r#"{"fov_deg":20.0,"profile":"clean"}"#).unwrap();
        let solve = |bytes: &[u8], kind: &str, stride: u32, o: &CString| -> String {
            let k = CString::new(kind).unwrap();
            let mut e: *mut c_char = std::ptr::null_mut();
            let out = unsafe {
                unisolver_solve_frame_json_opts(
                    solver,
                    bytes.as_ptr(),
                    bytes.len(),
                    1024,
                    768,
                    k.as_ptr(),
                    stride,
                    o.as_ptr(),
                    &mut e,
                )
            };
            assert!(!out.is_null(), "{kind}: {:?}", unsafe {
                e.as_ref().map(|_| CStr::from_ptr(e).to_string_lossy())
            });
            let s = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
            unsafe { unisolver_string_free(out) };
            s
        };

        // (1) LumaF32 passed straight through
        let f32_bytes: Vec<u8> = img.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let json = solve(&f32_bytes, "luma_f32", 0, &opts);
        assert!(json.contains("\"status\":\"Ok\""), "{json}");
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!((v["solution"]["ra_deg"].as_f64().unwrap() - 120.0).abs() < 0.2);
        assert_eq!(
            v["attempts"].as_array().unwrap().len(),
            1,
            "a known FOV must try exactly one rung"
        );

        // (2) Luma16 / Luma8: normalized to their ranges
        let maxv = img.iter().cloned().fold(0.0f32, f32::max).max(1.0);
        let u16_bytes: Vec<u8> = img
            .iter()
            .flat_map(|v| ((v / maxv * 65535.0) as u16).to_ne_bytes())
            .collect();
        assert!(solve(&u16_bytes, "luma16", 0, &opts).contains("\"status\":\"Ok\""));
        // 8-bit needs a percentile stretch: dividing by the brightest star quantizes faint stars
        // away (real 8-bit pipelines do not peak-normalize either). A fixture issue, not a solver one
        let mut sorted = img.clone();
        sorted.sort_by(f32::total_cmp);
        let hi = sorted[(sorted.len() as f64 * 0.999) as usize].max(1.0);
        let u8_px: Vec<u8> = img
            .iter()
            .map(|v| (v / hi * 255.0).clamp(0.0, 255.0) as u8)
            .collect();
        assert!(solve(&u8_px, "luma8", 0, &opts).contains("\"status\":\"Ok\""));

        // (3) A padded stride (normal for camera buffers: Android rowStride / iOS bytesPerRow)
        let pad = 96usize;
        let stride = 1024 + pad;
        let mut padded = vec![0u8; stride * 768];
        for y in 0..768 {
            padded[y * stride..y * stride + 1024].copy_from_slice(&u8_px[y * 1024..(y + 1) * 1024]);
        }
        assert!(solve(&padded, "luma8", stride as u32, &opts).contains("\"status\":\"Ok\""));

        // (4) Tracking: previous attitude as the hint, still without disk
        let quat = unisolver_core::test_support::wxyz(&q);
        let track = CString::new(format!(
            r#"{{"fov_deg":20.0,"profile":"clean","attitude_hint_wxyz":[{},{},{},{}]}}"#,
            quat[0], quat[1], quat[2], quat[3]
        ))
        .unwrap();
        assert!(solve(&f32_bytes, "luma_f32", 0, &track).contains("\"status\":\"Ok\""));

        // (5) Bad parameters: unknown kind / NULL pointer / buffer shorter than declared → errors, no crash
        let bad_kind = CString::new("rgb565").unwrap();
        let r = unsafe {
            unisolver_solve_frame_json_opts(
                solver,
                f32_bytes.as_ptr(),
                f32_bytes.len(),
                1024,
                768,
                bad_kind.as_ptr(),
                0,
                opts.as_ptr(),
                &mut err,
            )
        };
        assert!(r.is_null() && !err.is_null());
        unsafe { unisolver_string_free(err) };

        let k = CString::new("luma8").unwrap();
        let r = unsafe {
            unisolver_solve_frame_json_opts(
                solver,
                std::ptr::null(),
                0,
                1024,
                768,
                k.as_ptr(),
                0,
                opts.as_ptr(),
                &mut err,
            )
        };
        assert!(r.is_null() && !err.is_null());
        unsafe { unisolver_string_free(err) };

        let short = &u8_px[..1000];
        let r = unsafe {
            unisolver_solve_frame_json_opts(
                solver,
                short.as_ptr(),
                short.len(),
                1024,
                768,
                k.as_ptr(),
                0,
                opts.as_ptr(),
                &mut err,
            )
        };
        assert!(
            r.is_null() && !err.is_null(),
            "a short buffer must be an error"
        );
        let msg = unsafe { CStr::from_ptr(err) }.to_string_lossy().to_string();
        assert!(msg.contains("buffer"), "{msg}");
        unsafe { unisolver_string_free(err) };

        unsafe { unisolver_close(solver) };
    }

    #[test]
    fn errors_are_reported_not_crashed() {
        let mut err: *mut c_char = std::ptr::null_mut();
        let bad = CString::new("/nonexistent/db.bin").unwrap();
        let s = unsafe { unisolver_open(bad.as_ptr(), &mut err) };
        assert!(s.is_null() && !err.is_null());
        unsafe { unisolver_string_free(err) };
    }

    #[test]
    fn attributions_are_json_with_the_gaia_text() {
        let p = unisolver_attributions_json();
        assert!(!p.is_null());
        let json = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned();
        unsafe { unisolver_string_free(p) };
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), unisolver_core::data_attributions().len());
        assert!(json.contains("\"iau_constellations\""));
        for a in arr {
            for k in ["id", "name", "applies_to", "license", "text", "url"] {
                assert!(a[k].is_string(), "{k} missing in {a}");
            }
        }
        assert!(json.contains("mission Gaia"));
    }
}
