//! DTO layer: conversions between core types and FRB-translatable types.
//! The only boundary where FRB semantics may leak in (core knows nothing about FRB).
use flutter_rust_bridge::frb;
use unisolver_core as core;

// ── Frame ───────────────────────────────────────────────────────────────
pub enum PixelKindDto {
    Luma8,
    Luma16,
    LumaF32,
    Rgba8,
}

pub struct FrameDto {
    pub width: u32,
    pub height: u32,
    pub row_stride_bytes: Option<u32>,
    pub kind: PixelKindDto,
    /// Luma16/LumaF32 are packed little-endian (a Dart Uint8List.view is little-endian)
    pub bytes: Vec<u8>,
}

impl TryFrom<FrameDto> for core::Frame {
    type Error = core::CoreError;
    fn try_from(d: FrameDto) -> Result<Self, Self::Error> {
        let pixels = match d.kind {
            PixelKindDto::Luma8 => core::PixelData::Luma8(d.bytes),
            PixelKindDto::Rgba8 => core::PixelData::Rgba8(d.bytes),
            PixelKindDto::Luma16 => {
                if !d.bytes.len().is_multiple_of(2) {
                    return Err(core::CoreError::InvalidInput(
                        "Luma16 bytes not even".into(),
                    ));
                }
                core::PixelData::Luma16(
                    d.bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_le_bytes(*c))
                        .collect(),
                )
            }
            PixelKindDto::LumaF32 => {
                if !d.bytes.len().is_multiple_of(4) {
                    return Err(core::CoreError::InvalidInput(
                        "LumaF32 bytes not mult of 4".into(),
                    ));
                }
                core::PixelData::LumaF32(
                    d.bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|c| f32::from_le_bytes(*c))
                        .collect(),
                )
            }
        };
        Ok(core::Frame {
            width: d.width,
            height: d.height,
            row_stride_bytes: d.row_stride_bytes,
            pixels,
        })
    }
}

// ── Camera ──────────────────────────────────────────────────────────────
pub enum DistortionDto {
    None,
    Radial {
        k1: f64,
        k2: f64,
        k3: f64,
        p1: f64,
        p2: f64,
        center_x: Option<f64>,
        center_y: Option<f64>,
    },
    Polynomial {
        order: u32,
        scale: f64,
        a_coeffs: Vec<f64>,
        b_coeffs: Vec<f64>,
    },
}

#[derive(Clone)]
pub struct CameraParamsDto {
    pub focal_length_px: f64,
    pub pp_x: f64,
    pub pp_y: f64,
    pub parity_flip: bool,
    pub distortion: DistortionDto,
}

impl Clone for DistortionDto {
    fn clone(&self) -> Self {
        match self {
            Self::None => Self::None,
            Self::Radial {
                k1,
                k2,
                k3,
                p1,
                p2,
                center_x,
                center_y,
            } => Self::Radial {
                k1: *k1,
                k2: *k2,
                k3: *k3,
                p1: *p1,
                p2: *p2,
                center_x: *center_x,
                center_y: *center_y,
            },
            Self::Polynomial {
                order,
                scale,
                a_coeffs,
                b_coeffs,
            } => Self::Polynomial {
                order: *order,
                scale: *scale,
                a_coeffs: a_coeffs.clone(),
                b_coeffs: b_coeffs.clone(),
            },
        }
    }
}

impl From<core::CameraParams> for CameraParamsDto {
    fn from(c: core::CameraParams) -> Self {
        let distortion = match c.distortion {
            core::DistortionParams::None => DistortionDto::None,
            core::DistortionParams::Radial {
                k1,
                k2,
                k3,
                p1,
                p2,
                center,
            } => DistortionDto::Radial {
                k1,
                k2,
                k3,
                p1,
                p2,
                center_x: center.map(|c| c.0),
                center_y: center.map(|c| c.1),
            },
            core::DistortionParams::Polynomial {
                order,
                scale,
                a_coeffs,
                b_coeffs,
            } => DistortionDto::Polynomial {
                order,
                scale,
                a_coeffs,
                b_coeffs,
            },
        };
        Self {
            focal_length_px: c.focal_length_px,
            pp_x: c.principal_point.0,
            pp_y: c.principal_point.1,
            parity_flip: c.parity_flip,
            distortion,
        }
    }
}

impl TryFrom<CameraParamsDto> for core::CameraParams {
    type Error = core::CoreError;
    fn try_from(d: CameraParamsDto) -> Result<Self, Self::Error> {
        let distortion = match d.distortion {
            DistortionDto::None => core::DistortionParams::None,
            DistortionDto::Radial {
                k1,
                k2,
                k3,
                p1,
                p2,
                center_x,
                center_y,
            } => {
                let center = match (center_x, center_y) {
                    (Some(x), Some(y)) => Some((x, y)),
                    (None, None) => None,
                    _ => {
                        return Err(core::CoreError::InvalidInput(
                            "radial center_x/center_y must both be set or both absent".into(),
                        ))
                    }
                };
                core::DistortionParams::Radial {
                    k1,
                    k2,
                    k3,
                    p1,
                    p2,
                    center,
                }
            }
            DistortionDto::Polynomial {
                order,
                scale,
                a_coeffs,
                b_coeffs,
            } => core::DistortionParams::Polynomial {
                order,
                scale,
                a_coeffs,
                b_coeffs,
            },
        };
        Ok(core::CameraParams {
            focal_length_px: d.focal_length_px,
            principal_point: (d.pp_x, d.pp_y),
            parity_flip: d.parity_flip,
            distortion,
        })
    }
}

// ── Extraction profile / SolveOptions ─────────────────────────────────────
pub enum ExtractionProfileDto {
    Auto,
    PhoneJpeg,
    CleanSensor,
    CustomCcl { sigma: f32, max_centroids: u32 },
    CustomFast { sigma: f32, max_centroids: u32 },
}

impl From<ExtractionProfileDto> for core::ExtractionProfile {
    fn from(d: ExtractionProfileDto) -> Self {
        match d {
            ExtractionProfileDto::Auto => core::ExtractionProfile::Auto,
            ExtractionProfileDto::PhoneJpeg => core::ExtractionProfile::PhoneJpeg,
            ExtractionProfileDto::CleanSensor => core::ExtractionProfile::CleanSensor,
            ExtractionProfileDto::CustomCcl {
                sigma,
                max_centroids,
            } => core::ExtractionProfile::Custom(core::ExtractionOptions::Ccl {
                sigma_threshold: sigma,
                max_centroids: max_centroids as usize,
            }),
            ExtractionProfileDto::CustomFast {
                sigma,
                max_centroids,
            } => core::ExtractionProfile::Custom(core::ExtractionOptions::Fast {
                sigma_threshold: sigma,
                max_centroids: max_centroids as usize,
            }),
        }
    }
}

pub struct SolveOptionsDto {
    pub fov_estimate_deg: f32,
    pub fov_max_error_deg: Option<f32>,
    pub camera: Option<CameraParamsDto>,
    pub attitude_hint_wxyz: Option<[f32; 4]>,
    pub hint_uncertainty_deg: f32,
    pub strict_hint: bool,
    pub profile: ExtractionProfileDto,
    pub retry_alternate_profile: bool,
    /// Ladders only: after the staged search fails, search every rung exhaustively.
    /// Off by default so frames without stars fail in about two seconds.
    pub thorough: bool,
    pub match_threshold: f64,
    pub timeout_ms: Option<u64>,
    /// Observation time (Unix ms), reported back as `observationUnixMs` for the solar-system
    /// layer; it does not change the solution
    pub observation_unix_ms: Option<i64>,
    /// Ladders only: EXIF FocalLengthIn35mmFilm read by the app, for formats the engine does
    /// not decode (HEIC: decode with the platform and use `solveFrameAuto`). Its FOV is tried
    /// first as a hint (±15%) and the ladder still follows; ignored with a camera or a
    /// tracking hint. Files read their own EXIF.
    pub focal_length_35mm: Option<f32>,
}

impl SolveOptionsDto {
    /// Defaults shared with core::SolveOptions::new (PhoneJpeg, built-in σ, 5 s timeout)
    #[frb(sync)]
    pub fn defaults(fov_estimate_deg: f32) -> Self {
        Self {
            fov_estimate_deg,
            fov_max_error_deg: None,
            camera: None,
            attitude_hint_wxyz: None,
            hint_uncertainty_deg: 3.0,
            strict_hint: false,
            profile: ExtractionProfileDto::PhoneJpeg,
            retry_alternate_profile: true,
            thorough: false,
            match_threshold: 1e-5,
            timeout_ms: Some(5000),
            observation_unix_ms: None,
            focal_length_35mm: None,
        }
    }
}

impl TryFrom<SolveOptionsDto> for core::SolveOptions {
    type Error = core::CoreError;
    fn try_from(d: SolveOptionsDto) -> Result<Self, Self::Error> {
        let mut o = core::SolveOptions::new(d.fov_estimate_deg);
        o.fov_max_error_deg = d.fov_max_error_deg;
        o.camera = d.camera.map(core::CameraParams::try_from).transpose()?;
        o.attitude_hint = d.attitude_hint_wxyz;
        o.hint_uncertainty_deg = d.hint_uncertainty_deg;
        o.strict_hint = d.strict_hint;
        o.extraction = d.profile.into();
        o.retry_alternate_profile = d.retry_alternate_profile;
        o.thorough = d.thorough;
        o.match_threshold = d.match_threshold;
        o.timeout_ms = d.timeout_ms;
        o.observation_unix_ms = d.observation_unix_ms;
        o.focal_length_35mm = d.focal_length_35mm;
        Ok(o)
    }
}

// ── Results ───────────────────────────────────────────────────────────────
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum SolveStatusDto {
    Ok,
    NoMatch,
    Timeout,
    TooFew,
}

impl From<core::SolveStatus> for SolveStatusDto {
    fn from(s: core::SolveStatus) -> Self {
        match s {
            core::SolveStatus::Ok => Self::Ok,
            core::SolveStatus::NoMatch => Self::NoMatch,
            core::SolveStatus::Timeout => Self::Timeout,
            core::SolveStatus::TooFew => Self::TooFew,
        }
    }
}

pub struct TimingDto {
    pub extract_ms: f32,
    pub solve_ms: f32,
    pub total_ms: f32,
}

pub struct CentroidDto {
    pub x: f64,
    pub y: f64,
    pub mass: Option<f32>,
    /// Axis ratio (1.0 = round star); trailing or tracking error raises it
    pub elongation: Option<f32>,
}

pub struct MatchDto {
    pub centroid_index: u32,
    pub catalog_id: i64,
    pub x: f64,
    pub y: f64,
}

pub struct WcsDto {
    pub width: u32,
    pub height: u32,
    /// [cd11, cd12, cd21, cd22]
    pub cd: [f64; 4],
    pub crval_ra_deg: f64,
    pub crval_dec_deg: f64,
    pub theta_rad: f64,
    pub camera: CameraParamsDto,
}

impl From<core::Wcs> for WcsDto {
    fn from(w: core::Wcs) -> Self {
        Self {
            width: w.width,
            height: w.height,
            cd: [w.cd[0][0], w.cd[0][1], w.cd[1][0], w.cd[1][1]],
            crval_ra_deg: w.crval_deg[0],
            crval_dec_deg: w.crval_deg[1],
            theta_rad: w.theta_rad,
            camera: w.camera.into(),
        }
    }
}

impl TryFrom<WcsDto> for core::Wcs {
    type Error = core::CoreError;
    fn try_from(d: WcsDto) -> Result<Self, Self::Error> {
        let camera: core::CameraParams = d.camera.try_into()?;
        camera.validate(d.width, d.height)?;
        Ok(core::Wcs {
            width: d.width,
            height: d.height,
            cd: [[d.cd[0], d.cd[1]], [d.cd[2], d.cd[3]]],
            crval_deg: [d.crval_ra_deg, d.crval_dec_deg],
            theta_rad: d.theta_rad,
            camera,
        })
    }
}

pub struct SolvedGeometryDto {
    pub quat_icrs2cam_wxyz: [f32; 4],
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub roll_deg: f64,
    pub fov_deg: f32,
    pub num_matches: u32,
    pub rmse_arcsec: f32,
    pub p90_arcsec: f32,
    pub max_err_arcsec: f32,
    pub prob: f64,
    pub wcs: WcsDto,
    pub matched: Vec<MatchDto>,
}

pub struct SolveOutcomeDto {
    pub status: SolveStatusDto,
    pub solution: Option<SolvedGeometryDto>,
    pub centroids: Vec<CentroidDto>,
    pub timing: TimingDto,
    pub extraction_retried: bool,
    /// Star-shape diagnostic: median elongation of the brightest quarter (1.0 = round).
    /// **Relative, with no cross-device threshold**: it varies with pixel scale, PSF and
    /// exposure. Round stars on a telescope camera measure 1.3–1.4 and its untracked 3 s
    /// frames 1.8–1.9, while solvable phone frames span 1.13–2.51. Compare against a baseline
    /// from the same camera; never hard-code a threshold.
    pub median_elongation: Option<f32>,
    /// Observation time the solve used (Unix ms, UTC): the options', or for file entries the
    /// header's when it pins the zone (FITS DATE-OBS, EXIF with an offset). Pass it to
    /// `AnnotateOptionsDto.observationUnixMs` for the solar-system layer; null when neither
    /// gave one.
    pub observation_unix_ms: Option<i64>,
    /// Where the photo was taken, for file entries whose EXIF has a GPS position. Pass it to
    /// `AnnotateOptionsDto.observer` with the time (the moon's parallax reaches 1° without
    /// it); null for frames and files without a position.
    pub observer: Option<ObserverDto>,
}

impl From<core::SolveOutcome> for SolveOutcomeDto {
    fn from(o: core::SolveOutcome) -> Self {
        Self {
            status: o.status.into(),
            solution: o.solution.map(|g| SolvedGeometryDto {
                quat_icrs2cam_wxyz: g.quat_icrs2cam_wxyz,
                ra_deg: g.ra_deg,
                dec_deg: g.dec_deg,
                roll_deg: g.roll_deg,
                fov_deg: g.fov_deg,
                num_matches: g.num_matches,
                rmse_arcsec: g.rmse_arcsec,
                p90_arcsec: g.p90_arcsec,
                max_err_arcsec: g.max_err_arcsec,
                prob: g.prob,
                wcs: g.wcs.into(),
                matched: g
                    .matched
                    .into_iter()
                    .map(|m| MatchDto {
                        centroid_index: m.centroid_index as u32,
                        catalog_id: m.catalog_id,
                        x: m.x,
                        y: m.y,
                    })
                    .collect(),
            }),
            centroids: o
                .centroids
                .into_iter()
                .map(|c| CentroidDto {
                    x: c.x,
                    y: c.y,
                    mass: c.mass,
                    elongation: c.elongation,
                })
                .collect(),
            timing: TimingDto {
                extract_ms: o.timing.extract_ms,
                solve_ms: o.timing.solve_ms,
                total_ms: o.timing.total_ms,
            },
            extraction_retried: o.extraction_retried,
            median_elongation: o.median_elongation,
            observation_unix_ms: o.observation_unix_ms,
            observer: o.observer.map(Into::into),
        }
    }
}

// ── Annotation ────────────────────────────────────────────────────────────

/// Observer location: needed by the moon's parallax correction and the satellite layer.
pub struct ObserverDto {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
}

impl From<core::Observer> for ObserverDto {
    fn from(o: core::Observer) -> Self {
        Self {
            lat_deg: o.lat_deg,
            lon_deg: o.lon_deg,
            alt_m: o.alt_m,
        }
    }
}

impl From<ObserverDto> for core::Observer {
    fn from(o: ObserverDto) -> Self {
        Self {
            lat_deg: o.lat_deg,
            lon_deg: o.lon_deg,
            alt_m: o.alt_m,
        }
    }
}

/// Topocentric apparent position of one satellite at a given time.
pub struct SatellitePosDto {
    pub name: String,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub range_km: f64,
    /// Below the horizon means hidden by the Earth: do not draw
    pub above_horizon: bool,
}

#[frb]
pub struct AnnotateOptionsDto {
    pub star_max_mag: Option<f32>,
    pub max_stars: u32,
    pub include_star_names: bool,
    pub include_dso: bool,
    /// Drops fainter deep-sky objects, or those without a magnitude, except outlined ones
    pub dso_max_mag: Option<f32>,
    /// Projected outlines of extended objects (`DsoAnnotationDto.outlines`)
    pub dso_outlines: bool,
    /// Highest outline level: 1 = faint outer edge … 3 = bright core
    pub max_outline_level: u8,
    pub include_solar_system: bool,
    pub observation_unix_ms: Option<i64>,
    /// Observer: when given, the **moon** gets its topocentric parallax (without it the moon can
    /// be off by 1°, two lunar diameters); required by the satellite layer. Planets and the sun
    /// are unaffected (parallax ≤ 0.003°).
    pub observer: Option<ObserverDto>,
    /// TLE text for the satellite layer (fetched by the caller; the engine never goes online);
    /// also needs observation_unix_ms and observer.
    pub satellite_tle: Option<String>,
    /// Language code: `en` / `zh_cn` / `zh_tw` / `ja` / `ko` / `fr` / `de` / `es` / `it` /
    /// `ru` / `pl` / `hu` / `ro` (as in the names pack; `UniAnnotator.languages()` lists them).
    /// Lenient: `zh-CN`, `zh_Hans` and `zh` map to Simplified Chinese; English when missing.
    pub language: String,
    /// Constellation figures (the IAU charts' lines) with their names
    /// (`AnnotationsDto.constellations`); needs the constellation pack
    #[frb(default = false)]
    pub include_constellations: bool,
    /// IAU constellation boundaries (`AnnotationsDto.boundaries`); needs the constellation pack
    #[frb(default = false)]
    pub constellation_boundaries: bool,
    /// Equatorial grid (`AnnotationsDto.grid`, J2000 right ascension and declination)
    #[frb(default = false)]
    pub equatorial_grid: bool,
    /// Horizontal grid (apparent altitude and azimuth, with the horizon); needs
    /// `observationUnixMs` and `observer`
    #[frb(default = false)]
    pub horizontal_grid: bool,
    /// Screen pixels between grid lines (default 150)
    pub grid_spacing_px: Option<f64>,
    /// What the app shows right now: pass it and lines and labels follow the zoom (grid
    /// spacing, sampling, simplification to half a screen pixel, labels on the visible edges).
    /// Annotate again when it changes; null means the whole image at scale 1.
    pub viewport: Option<ViewportDto>,
}

/// The visible part of the image and the zoom.
pub struct ViewportDto {
    /// Visible region in image pixels (top-left origin)
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Screen pixels per image pixel
    pub scale: f64,
}

impl From<ViewportDto> for core::Viewport {
    fn from(v: ViewportDto) -> Self {
        Self {
            x: v.x,
            y: v.y,
            width: v.width,
            height: v.height,
            scale: v.scale,
        }
    }
}

impl AnnotateOptionsDto {
    #[frb(sync)]
    pub fn defaults() -> Self {
        Self {
            star_max_mag: Some(6.5),
            max_stars: 300,
            include_star_names: true,
            include_dso: true,
            dso_max_mag: None,
            dso_outlines: true,
            max_outline_level: 3,
            include_solar_system: true,
            observation_unix_ms: None,
            observer: None,
            satellite_tle: None,
            language: "en".to_string(),
            include_constellations: false,
            constellation_boundaries: false,
            equatorial_grid: false,
            horizontal_grid: false,
            grid_spacing_px: None,
            viewport: None,
        }
    }
}

impl From<AnnotateOptionsDto> for core::AnnotateOptions {
    fn from(d: AnnotateOptionsDto) -> Self {
        Self {
            star_max_mag: d.star_max_mag,
            max_stars: d.max_stars as usize,
            include_star_names: d.include_star_names,
            include_dso: d.include_dso,
            dso_max_mag: d.dso_max_mag,
            dso_outlines: d.dso_outlines,
            max_outline_level: d.max_outline_level,
            include_solar_system: d.include_solar_system,
            observation_unix_ms: d.observation_unix_ms,
            observer: d.observer.map(Into::into),
            satellite_tle: d.satellite_tle,
            language: d.language,
            include_constellations: d.include_constellations,
            constellation_boundaries: d.constellation_boundaries,
            equatorial_grid: d.equatorial_grid,
            horizontal_grid: d.horizontal_grid,
            grid_spacing_px: d.grid_spacing_px,
            viewport: d.viewport.map(Into::into),
        }
    }
}

pub enum DsoKindDto {
    Galaxy,
    OpenCluster,
    GlobularCluster,
    Nebula,
    PlanetaryNebula,
    HiiRegion,
    SupernovaRemnant,
    DarkNebula,
    ClusterWithNebula,
    Association,
    Other,
}

impl From<core::dso::DsoKind> for DsoKindDto {
    fn from(k: core::dso::DsoKind) -> Self {
        use core::dso::DsoKind as K;
        match k {
            K::Galaxy => Self::Galaxy,
            K::OpenCluster => Self::OpenCluster,
            K::GlobularCluster => Self::GlobularCluster,
            K::Nebula => Self::Nebula,
            K::PlanetaryNebula => Self::PlanetaryNebula,
            K::HiiRegion => Self::HiiRegion,
            K::SupernovaRemnant => Self::SupernovaRemnant,
            K::DarkNebula => Self::DarkNebula,
            K::ClusterWithNebula => Self::ClusterWithNebula,
            K::Association => Self::Association,
            K::Other => Self::Other,
        }
    }
}

pub struct StarAnnotationDto {
    pub x: f64,
    pub y: f64,
    pub mag: f32,
    pub catalog_id: i64,
}

pub struct NamedStarAnnotationDto {
    pub x: f64,
    pub y: f64,
    pub mag: f32,
    pub hip: u32,
    pub name: String,
}

pub struct DsoAnnotationDto {
    pub x: f64,
    pub y: f64,
    pub semi_major_px: f64,
    pub semi_minor_px: f64,
    /// None = orientation unknown: draw a circle of the semi-major axis, never guess an angle
    pub angle_deg: Option<f64>,
    pub designation: String,
    pub common_name: Option<String>,
    pub kind: DsoKindDto,
    pub mag: Option<f32>,
    /// Outlines in pixels, outermost level first (empty for most objects). For an outlined
    /// object `x`/`y` may lie outside the image while part of the outline is inside.
    pub outlines: Vec<DsoOutlineDto>,
}

/// One brightness level of a projected outline.
pub struct DsoOutlineDto {
    /// 1 = faint outer edge … 3 = bright core
    pub level: u8,
    pub contours: Vec<OutlineContourDto>,
}

/// A projected contour in pixels (top-left origin); a closed contour's last point joins its first.
pub struct OutlineContourDto {
    pub closed: bool,
    /// Interleaved x, y
    pub points: Vec<f64>,
}

pub struct LayerAvailabilityDto {
    pub catalog_stars: bool,
    pub named_stars: bool,
    pub dso: bool,
    pub solar_system: bool,
    pub satellites: bool,
    /// Constellation figures and boundaries
    pub constellations: bool,
    /// Coordinate grids
    pub grid: bool,
    /// Why a layer is unavailable or degraded, as `(layer, message)`: show it rather than an
    /// empty layer
    pub reasons: Vec<(String, String)>,
}

/// A constellation with part of its figure in the frame.
pub struct ConstellationAnnotationDto {
    /// IAU abbreviation (`Ori`)
    pub abbr: String,
    /// Name in the requested language (from the names pack), else the IAU name
    pub name: String,
    /// Label position; null when no vertex of the figure is in the frame
    pub label_x: Option<f64>,
    pub label_y: Option<f64>,
    /// Figure polylines in pixels, interleaved x, y. They may run past the frame edge.
    pub lines: Vec<Vec<f64>>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum GridSystemDto {
    /// J2000 right ascension and declination
    Equatorial,
    /// Apparent altitude and azimuth (from north through east)
    Horizontal,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum GridKindDto {
    Ra,
    Dec,
    Alt,
    Az,
    /// Altitude 0°
    Horizon,
}

/// The edge of the visible region a label sits on: nudge the text inward from it.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum GridEdgeDto {
    Left,
    Right,
    Top,
    Bottom,
    /// The line stays inside the region: mid-line
    Inside,
}

pub struct GridLabelDto {
    pub x: f64,
    pub y: f64,
    /// Direction of the line there, counter-clockwise from +x (image y down), in (−90°, 90°]
    pub angle_deg: f64,
    pub edge: GridEdgeDto,
}

/// One grid line with its value and label.
pub struct GridLineDto {
    pub system: GridSystemDto,
    pub kind: GridKindDto,
    /// Right ascension or azimuth in [0, 360), declination or altitude in degrees
    pub value_deg: f64,
    /// The value as charts print it: `16h30m`, `−20°30′`, `180°`
    pub text: String,
    /// Compass point for azimuths on a multiple of 45° (`N`, `NE`, … `NW`): localize it
    pub cardinal: Option<String>,
    /// Pixel polylines, interleaved x, y; they may run past the visible edge
    pub lines: Vec<Vec<f64>>,
    pub label: Option<GridLabelDto>,
}

/// The constellation a position is in (`UniAnnotator.constellationAt`).
pub struct ConstellationNameDto {
    /// IAU abbreviation (`Ori`)
    pub abbr: String,
    /// Name in the requested language (from the names pack), else the IAU name
    pub name: String,
}

impl From<core::constellations::ConstellationName> for ConstellationNameDto {
    fn from(c: core::constellations::ConstellationName) -> Self {
        Self {
            abbr: c.abbr,
            name: c.name,
        }
    }
}

/// A stretch of IAU boundary in the frame.
pub struct BoundaryAnnotationDto {
    /// IAU abbreviations of the constellations on either side
    pub between: Vec<String>,
    /// Interleaved x, y
    pub points: Vec<f64>,
}

pub struct SolarAnnotationDto {
    pub x: f64,
    pub y: f64,
    pub name: String,
    pub angular_radius_px: Option<f64>,
}

/// Satellite passes (above the horizon only; below it they are hidden by the Earth)
pub struct SatelliteAnnotationDto {
    pub x: f64,
    pub y: f64,
    pub name: String,
    pub range_km: f64,
}

pub struct AnnotationsDto {
    pub stars: Vec<StarAnnotationDto>,
    pub named_stars: Vec<NamedStarAnnotationDto>,
    pub objects: Vec<DsoAnnotationDto>,
    pub solar: Vec<SolarAnnotationDto>,
    pub satellites: Vec<SatelliteAnnotationDto>,
    pub constellations: Vec<ConstellationAnnotationDto>,
    pub boundaries: Vec<BoundaryAnnotationDto>,
    pub grid: Vec<GridLineDto>,
    pub layers: LayerAvailabilityDto,
}

impl From<core::Annotations> for AnnotationsDto {
    fn from(a: core::Annotations) -> Self {
        Self {
            stars: a
                .stars
                .into_iter()
                .map(|s| StarAnnotationDto {
                    x: s.x,
                    y: s.y,
                    mag: s.mag,
                    catalog_id: s.catalog_id,
                })
                .collect(),
            named_stars: a
                .named_stars
                .into_iter()
                .map(|n| NamedStarAnnotationDto {
                    x: n.x,
                    y: n.y,
                    mag: n.mag,
                    hip: n.hip,
                    name: n.name,
                })
                .collect(),
            objects: a
                .objects
                .into_iter()
                .map(|o| DsoAnnotationDto {
                    x: o.x,
                    y: o.y,
                    semi_major_px: o.semi_major_px,
                    semi_minor_px: o.semi_minor_px,
                    angle_deg: o.angle_deg,
                    designation: o.designation,
                    common_name: o.common_name,
                    kind: o.kind.into(),
                    mag: o.mag,
                    outlines: o
                        .outlines
                        .into_iter()
                        .map(|l| DsoOutlineDto {
                            level: l.level,
                            contours: l
                                .contours
                                .into_iter()
                                .map(|c| OutlineContourDto {
                                    closed: c.closed,
                                    points: c.points.into_iter().flatten().collect(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
            solar: a
                .solar
                .into_iter()
                .map(|s| SolarAnnotationDto {
                    x: s.x,
                    y: s.y,
                    name: s.name,
                    angular_radius_px: s.angular_radius_px,
                })
                .collect(),
            satellites: a
                .satellites
                .into_iter()
                .map(|s| SatelliteAnnotationDto {
                    x: s.x,
                    y: s.y,
                    name: s.name,
                    range_km: s.range_km,
                })
                .collect(),
            constellations: a
                .constellations
                .into_iter()
                .map(|c| ConstellationAnnotationDto {
                    abbr: c.abbr,
                    name: c.name,
                    label_x: c.label.map(|p| p[0]),
                    label_y: c.label.map(|p| p[1]),
                    lines: c
                        .lines
                        .into_iter()
                        .map(|l| l.into_iter().flatten().collect())
                        .collect(),
                })
                .collect(),
            boundaries: a
                .boundaries
                .into_iter()
                .map(|b| BoundaryAnnotationDto {
                    between: b.between.to_vec(),
                    points: b.points.into_iter().flatten().collect(),
                })
                .collect(),
            grid: a
                .grid
                .into_iter()
                .map(|g| GridLineDto {
                    system: match g.system {
                        core::GridSystem::Equatorial => GridSystemDto::Equatorial,
                        core::GridSystem::Horizontal => GridSystemDto::Horizontal,
                    },
                    kind: match g.kind {
                        core::GridKind::Ra => GridKindDto::Ra,
                        core::GridKind::Dec => GridKindDto::Dec,
                        core::GridKind::Alt => GridKindDto::Alt,
                        core::GridKind::Az => GridKindDto::Az,
                        core::GridKind::Horizon => GridKindDto::Horizon,
                    },
                    value_deg: g.value_deg,
                    text: g.text,
                    cardinal: g.cardinal,
                    lines: g
                        .lines
                        .into_iter()
                        .map(|l| l.into_iter().flatten().collect())
                        .collect(),
                    label: g.label.map(|l| GridLabelDto {
                        x: l.x,
                        y: l.y,
                        angle_deg: l.angle_deg,
                        edge: match l.edge {
                            core::GridEdge::Left => GridEdgeDto::Left,
                            core::GridEdge::Right => GridEdgeDto::Right,
                            core::GridEdge::Top => GridEdgeDto::Top,
                            core::GridEdge::Bottom => GridEdgeDto::Bottom,
                            core::GridEdge::Inside => GridEdgeDto::Inside,
                        },
                    }),
                })
                .collect(),
            layers: LayerAvailabilityDto {
                catalog_stars: a.layers.catalog_stars,
                named_stars: a.layers.named_stars,
                dso: a.layers.dso,
                solar_system: a.layers.solar_system,
                satellites: a.layers.satellites,
                constellations: a.layers.constellations,
                grid: a.layers.grid,
                reasons: a.layers.reasons,
            },
        }
    }
}

// ── FOV ladder / calibration ──────────────────────────────────────────────
pub struct FovPresetDto {
    pub fov_deg: f32,
    pub max_error_deg: f32,
}

pub struct FovAttemptDto {
    pub fov_deg: f32,
    pub status: SolveStatusDto,
    pub solve_ms: f32,
}

impl From<core::FovAttempt> for FovAttemptDto {
    fn from(a: core::FovAttempt) -> Self {
        Self {
            fov_deg: a.fov_deg,
            status: a.status.into(),
            solve_ms: a.solve_ms,
        }
    }
}

pub struct LadderOutcomeDto {
    pub outcome: SolveOutcomeDto,
    pub attempts: Vec<FovAttemptDto>,
}

/// A registered tier (pool routing). Named as in the manifest, so a tier manager can match
/// installed and available tiers.
pub struct TierInfoDto {
    pub name: String,
    pub path: String,
    pub min_fov_deg: f32,
    pub max_fov_deg: f32,
    pub num_stars: u64,
    pub num_patterns: u32,
    pub star_max_magnitude: f32,
}

impl From<core::TierInfo> for TierInfoDto {
    fn from(t: core::TierInfo) -> Self {
        Self {
            name: t.name,
            path: t.path,
            min_fov_deg: t.min_fov_deg,
            max_fov_deg: t.max_fov_deg,
            num_stars: t.num_stars,
            num_patterns: t.num_patterns,
            star_max_magnitude: t.star_max_magnitude,
        }
    }
}

/// One cross-tier attempt (a `FovAttemptDto` plus the tier it used).
pub struct PoolAttemptDto {
    pub db: String,
    pub fov_deg: f32,
    pub status: SolveStatusDto,
    pub solve_ms: f32,
}

impl From<core::PoolAttempt> for PoolAttemptDto {
    fn from(a: core::PoolAttempt) -> Self {
        Self {
            db: a.db,
            fov_deg: a.fov_deg,
            status: a.status.into(),
            solve_ms: a.solve_ms,
        }
    }
}

pub struct PoolOutcomeDto {
    pub outcome: SolveOutcomeDto,
    pub attempts: Vec<PoolAttemptDto>,
    /// Tier that solved it (null on failure). Annotate with this tier: narrow tiers are denser
    pub db: Option<String>,
    /// Extractions actually performed (evidence of reuse across tiers)
    pub extract_count: u32,
}

impl From<core::PoolOutcome> for PoolOutcomeDto {
    fn from(r: core::PoolOutcome) -> Self {
        Self {
            outcome: r.outcome.into(),
            attempts: r.attempts.into_iter().map(Into::into).collect(),
            db: r.db,
            extract_count: r.extract_count as u32,
        }
    }
}

pub enum CalibModelDto {
    Radial,
    Polynomial { order: u8 },
}

pub struct CalibrationReportDto {
    pub camera: CameraParamsDto,
    pub rmse_before_px: f64,
    pub rmse_after_px: f64,
    pub n_inliers: u32,
    pub n_outliers: u32,
    pub frames_used: u32,
}

impl From<core::CalibrationReport> for CalibrationReportDto {
    fn from(r: core::CalibrationReport) -> Self {
        Self {
            camera: r.camera.into(),
            rmse_before_px: r.rmse_before_px,
            rmse_after_px: r.rmse_after_px,
            n_inliers: r.n_inliers as u32,
            n_outliers: r.n_outliers as u32,
            frames_used: r.frames_used as u32,
        }
    }
}

pub struct DbPropertiesDto {
    pub min_fov_deg: f32,
    pub max_fov_deg: f32,
    pub num_stars: u64,
    pub num_patterns: u32,
    pub star_max_magnitude: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_dto_luma16_little_endian_roundtrip() {
        let dto = FrameDto {
            width: 2,
            height: 1,
            row_stride_bytes: None,
            kind: PixelKindDto::Luma16,
            bytes: vec![0x34, 0x12, 0x00, 0xFF], // LE: 0x1234, 0xFF00
        };
        let f: core::Frame = dto.try_into().unwrap();
        let l = f.to_luma_f32().unwrap();
        assert_eq!(l, vec![0x1234 as f32, 0xFF00 as f32]);
    }
    #[test]
    fn camera_dto_roundtrip_radial() {
        let c = core::CameraParams {
            focal_length_px: 1262.7,
            principal_point: (959.5, 539.5),
            parity_flip: false,
            distortion: core::DistortionParams::Radial {
                k1: -2e-8,
                k2: 0.0,
                k3: 0.0,
                p1: 1e-9,
                p2: 0.0,
                center: Some((959.1, 566.2)),
            },
        };
        let dto: CameraParamsDto = c.clone().into();
        let back: core::CameraParams = dto.try_into().unwrap();
        assert_eq!(c, back);
    }
    #[test]
    fn solve_options_dto_defaults_map_to_core_defaults() {
        let o: core::SolveOptions = SolveOptionsDto::defaults(45.0).try_into().unwrap();
        assert_eq!(o.fov_estimate_deg, 45.0);
        assert!(o.retry_alternate_profile);
        assert!(matches!(o.extraction, core::ExtractionProfile::PhoneJpeg));
    }
    #[test]
    fn bad_frame_bytes_err_not_panic() {
        let dto = FrameDto {
            width: 3,
            height: 1,
            row_stride_bytes: None,
            kind: PixelKindDto::Luma16,
            bytes: vec![0, 1, 2],
        };
        assert!(core::Frame::try_from(dto).is_err());
    }
}
