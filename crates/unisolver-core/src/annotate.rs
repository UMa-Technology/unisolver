use crate::{outcome::Wcs, Result, Solver};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tetra3::SolverDatabase;

/// Language code matching a column of the names pack (`en` / `zh_cn` / `zh_tw` / `ja` / …).
/// Lenient: `zh-CN`, `zh_Hans` and `zh` all map to their column (see `NamesPack::resolve_language`).
pub type LanguageCode = String;

/// Whether the code is Chinese: used only to decide whether to fall back to the
/// catalog's Chinese-name column when the names pack lacks an object.
fn is_chinese(code: &str) -> bool {
    code.to_ascii_lowercase().starts_with("zh")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotateOptions {
    pub star_max_mag: Option<f32>,
    pub max_stars: usize,
    pub include_star_names: bool,
    pub include_dso: bool,
    /// Drops deep-sky objects fainter than this, or without a magnitude, except outlined
    /// ones (extended nebulae rarely have a magnitude; being outlined makes them notable)
    pub dso_max_mag: Option<f32>,
    /// Projected outlines for extended objects (see `DsoAnnotation::outlines`)
    #[serde(default = "default_true")]
    pub dso_outlines: bool,
    /// Highest outline level to return: 1 is the faint outer edge, 3 the bright core
    #[serde(default = "default_outline_level")]
    pub max_outline_level: u8,
    /// Solar-system layer (planets, moon, sun). Needs observation_unix_ms; unavailable without it.
    pub include_solar_system: bool,
    /// Observation time (Unix ms), required by the solar-system and satellite layers.
    pub observation_unix_ms: Option<i64>,
    /// Observer location. When given, the **moon** gets its topocentric parallax; without
    /// it the moon is geocentric and can be off by **up to 1° (two lunar diameters)**.
    /// Planet and sun parallax is ≤ 0.003° either way.
    pub observer: Option<crate::ephemeris::Observer>,
    /// TLE text for the satellite layer (two- or three-line sets). **The engine never
    /// goes online**: fetch TLEs yourself (CelesTrak etc.). Also needs
    /// observation_unix_ms and observer.
    pub satellite_tle: Option<String>,
    /// Language code (`en` / `zh_cn` / `ja` / …); English when the pack lacks it.
    /// Available languages come from the names pack, see `Annotator::languages()`.
    pub language: LanguageCode,
    /// Constellation figures (the IAU charts' lines) with their names; needs the
    /// constellation pack (see `Annotator::with_constellations`)
    #[serde(default)]
    pub include_constellations: bool,
    /// IAU constellation boundaries; needs the constellation pack
    #[serde(default)]
    pub constellation_boundaries: bool,
    /// Equatorial grid (J2000 right ascension and declination)
    #[serde(default)]
    pub equatorial_grid: bool,
    /// Horizontal grid (apparent altitude and azimuth, with the horizon); needs
    /// `observation_unix_ms` and `observer`
    #[serde(default)]
    pub horizontal_grid: bool,
    /// Screen pixels between grid lines (default 150); the step is the finest round value
    /// at least this far apart at the viewport's scale
    #[serde(default)]
    pub grid_spacing_px: Option<f64>,
    /// What the app shows right now (visible image region and zoom). Lines and labels follow
    /// it: grid spacing, sampling, simplification to half a screen pixel, labels on the
    /// visible edges. None means the whole image at scale 1. Point layers (stars, DSO,
    /// planets) are unaffected.
    #[serde(default)]
    pub viewport: Option<crate::sky::Viewport>,
}
impl Default for AnnotateOptions {
    fn default() -> Self {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarAnnotation {
    pub x: f64,
    pub y: f64,
    pub mag: f32,
    pub catalog_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedStarAnnotation {
    pub x: f64,
    pub y: f64,
    pub mag: f32,
    pub hip: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DsoAnnotation {
    pub x: f64,
    pub y: f64,
    pub semi_major_px: f64,
    pub semi_minor_px: f64,
    /// None = orientation unknown: draw a circle of the semi-major axis, never guess an angle
    pub angle_deg: Option<f64>,
    pub designation: String,
    pub common_name: Option<String>,
    pub kind: crate::dso::DsoKind,
    pub mag: Option<f32>,
    /// Outlines in pixels, outermost level first (empty for most objects). For an outlined
    /// object `x`/`y` may lie outside the image while part of the outline is inside.
    pub outlines: Vec<DsoOutline>,
}

/// One brightness level of a projected outline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DsoOutline {
    /// 1 = faint outer edge … 3 = bright core
    pub level: u8,
    pub contours: Vec<OutlineContour>,
}

/// A projected contour in pixel coordinates (top-left origin); a closed contour's last
/// point joins its first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineContour {
    pub closed: bool,
    pub points: Vec<[f64; 2]>,
}

fn default_true() -> bool {
    true
}

fn default_outline_level() -> u8 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolarAnnotation {
    pub x: f64,
    pub y: f64,
    pub name: String,
    /// Apparent radius in pixels; None = point-like (draw a small fixed marker)
    pub angular_radius_px: Option<f64>,
}

/// Satellite pass annotation, **above the horizon only**: a satellite below it is behind
/// the Earth even when its RA/Dec falls in the field (use
/// `satellites::satellite_positions` for everything).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatelliteAnnotation {
    pub x: f64,
    pub y: f64,
    pub name: String,
    /// Topocentric range (km), usable as a label
    pub range_km: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayerAvailability {
    pub catalog_stars: bool,
    pub named_stars: bool,
    pub dso: bool,
    pub solar_system: bool,
    pub satellites: bool,
    /// Constellation figures and boundaries
    #[serde(default)]
    pub constellations: bool,
    /// Coordinate grids
    #[serde(default)]
    pub grid: bool,
    pub reasons: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotations {
    pub stars: Vec<StarAnnotation>,
    pub named_stars: Vec<NamedStarAnnotation>,
    pub objects: Vec<DsoAnnotation>,
    pub solar: Vec<SolarAnnotation>,
    pub satellites: Vec<SatelliteAnnotation>,
    /// Constellations with part of their figure in the frame (`include_constellations`)
    #[serde(default)]
    pub constellations: Vec<crate::constellations::ConstellationAnnotation>,
    /// IAU boundary stretches in the frame (`constellation_boundaries`)
    #[serde(default)]
    pub boundaries: Vec<crate::constellations::BoundaryAnnotation>,
    /// Coordinate grid lines (`equatorial_grid`, `horizontal_grid`)
    #[serde(default)]
    pub grid: Vec<crate::grid::GridLineAnnotation>,
    pub layers: LayerAvailability,
}

pub struct Annotator {
    db: Arc<SolverDatabase>,
    dso: Option<crate::dso::DsoCatalog>,
    dso_error: Option<String>,
    names: Option<crate::names_pack::NamesPack>,
    names_error: Option<String>,
    constellations: Option<crate::constellations::Loaded>,
    constellations_error: String,
}

impl Solver {
    /// `names_path`: multilingual names pack (`UNAM`). Without it you get English names
    /// and the catalog's Chinese names; every layer still works, only localization is reduced.
    pub fn annotator(&self, dso_path: Option<&str>, names_path: Option<&str>) -> Result<Annotator> {
        let (dso, dso_error) = match dso_path {
            None => (None, Some("no DSO catalog configured".to_string())),
            Some(p) => match crate::dso::DsoCatalog::open(p) {
                Ok(c) => (Some(c), None),
                Err(e) => (None, Some(e.to_string())),
            },
        };
        let (names, names_error) = match names_path {
            None => (
                None,
                Some("no names pack configured (English names only)".to_string()),
            ),
            Some(p) => match crate::names_pack::NamesPack::open(p) {
                Ok(c) => (Some(c), None),
                Err(e) => (None, Some(e.to_string())),
            },
        };
        Ok(Annotator {
            db: self.db().clone(),
            dso,
            dso_error,
            names,
            names_error,
            constellations: None,
            constellations_error: "no constellation pack configured".into(),
        })
    }
}

impl Annotator {
    /// Loads the constellation pack (`UCON`) for the figure and boundary layers. Like the
    /// DSO catalog and the names pack, a missing or broken file does not fail: the layers
    /// report themselves unavailable, with the reason in `layers.reasons`.
    pub fn with_constellations(mut self, path: Option<&str>) -> Self {
        if let Some(p) = path {
            let _ = self.load_constellations(p);
        }
        self
    }

    /// As [`Self::with_constellations`] on an existing annotator, also returning the error
    /// (the layers then report it too).
    pub fn load_constellations(&mut self, path: &str) -> Result<()> {
        match crate::constellations::ConstellationPack::open(path) {
            Ok(p) => {
                self.constellations = Some(crate::constellations::Loaded::new(p));
                Ok(())
            }
            Err(e) => {
                self.constellations = None;
                self.constellations_error = e.to_string();
                Err(e)
            }
        }
    }

    /// Languages in the names pack (empty = no pack, English only).
    pub fn languages(&self) -> Vec<String> {
        self.names
            .as_ref()
            .map(|n| n.languages.clone())
            .unwrap_or_default()
    }

    /// The constellation containing J2000 `(ra, dec)` in degrees, named in `language` (as
    /// [`AnnotateOptions::language`]). For the frame centre pass the solve's centre; for a
    /// point on the image, convert it with [`Wcs::pixels_to_sky`] first. None without a
    /// constellation pack (`annotate` reports why in `layers.reasons`).
    pub fn constellation_at(
        &self,
        ra_deg: f64,
        dec_deg: f64,
        language: &str,
    ) -> Option<crate::constellations::ConstellationName> {
        let pack = &self.constellations.as_ref()?.pack;
        let c = &pack.constellations[pack.index_at(ra_deg, dec_deg)?];
        let lang = self
            .names
            .as_ref()
            .and_then(|n| n.resolve_language(language));
        Some(crate::constellations::ConstellationName {
            abbr: c.abbr.clone(),
            name: self
                .localized(&format!("CON {}", c.abbr), lang, Some(&c.name))
                .unwrap_or_else(|| c.name.clone()),
        })
    }

    /// Localized name: names pack (requested language → English), then the caller's fallback.
    fn localized(&self, key: &str, lang: Option<usize>, fallback: Option<&str>) -> Option<String> {
        self.names
            .as_ref()
            .and_then(|n| n.name(key, lang))
            .map(str::to_string)
            .or_else(|| fallback.map(str::to_string))
    }
}

/// Projects a record's outlines to pixels, up to `max_level`. A contour with a vertex that
/// does not project (behind the camera) is dropped whole, as is one entirely off the image;
/// the rest are simplified to `tol` image pixels (half a screen pixel).
fn project_outlines(
    wcs: &Wcs,
    r: &crate::dso::DsoRecord,
    max_level: u8,
    tol: f64,
) -> Vec<DsoOutline> {
    let (w, h) = (wcs.width as f64, wcs.height as f64);
    r.outlines
        .iter()
        .filter(|l| l.level <= max_level)
        .filter_map(|l| {
            let contours: Vec<OutlineContour> = l
                .contours
                .iter()
                .filter_map(|c| {
                    let points = c
                        .vertices
                        .iter()
                        .map(|&(ra, dec)| {
                            wcs.world_to_pixel(ra as f64, dec as f64)
                                .map(|(x, y)| [x, y])
                        })
                        .collect::<Option<Vec<_>>>()?;
                    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                    for p in &points {
                        (x0, y0) = (x0.min(p[0]), y0.min(p[1]));
                        (x1, y1) = (x1.max(p[0]), y1.max(p[1]));
                    }
                    if x1 < 0.0 || y1 < 0.0 || x0 >= w || y0 >= h {
                        return None;
                    }
                    let points = simplify(&points, tol);
                    (points.len() >= if c.closed { 3 } else { 2 }).then_some(OutlineContour {
                        closed: c.closed,
                        points,
                    })
                })
                .collect();
            (!contours.is_empty()).then_some(DsoOutline {
                level: l.level,
                contours,
            })
        })
        .collect()
}

/// Ramer–Douglas–Peucker simplification of a polyline, keeping both ends.
pub(crate) fn simplify(points: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    fn rdp(p: &[[f64; 2]], tol: f64, keep: &mut [bool], lo: usize, hi: usize) {
        if hi <= lo + 1 {
            return;
        }
        let (a, b) = (p[lo], p[hi]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        let (mut far, mut dmax) = (lo, 0.0);
        for (i, q) in p.iter().enumerate().take(hi).skip(lo + 1) {
            let d = if len == 0.0 {
                (q[0] - a[0]).hypot(q[1] - a[1])
            } else {
                ((q[0] - a[0]) * dy - (q[1] - a[1]) * dx).abs() / len
            };
            if d > dmax {
                (far, dmax) = (i, d);
            }
        }
        if dmax > tol {
            keep[far] = true;
            rdp(p, tol, keep, lo, far);
            rdp(p, tol, keep, far, hi);
        }
    }
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    let last = points.len() - 1;
    (keep[0], keep[last]) = (true, true);
    rdp(points, tol, &mut keep, 0, last);
    points
        .iter()
        .zip(keep)
        .filter_map(|(p, k)| k.then_some(*p))
        .collect()
}

/// Position angle (north through east, degrees) → image angle (counter-clockwise from +x):
/// project small offsets north and east of the object and combine them into the axis.
fn pa_to_image_angle(wcs: &Wcs, ra_deg: f64, dec_deg: f64, pa_deg: f64) -> Option<f64> {
    const E: f64 = 0.05;
    let p0 = wcs.world_to_pixel(ra_deg, dec_deg)?;
    let pn = wcs.world_to_pixel(ra_deg, (dec_deg + E).min(89.999))?;
    let pe = wcs.world_to_pixel(ra_deg + E / dec_deg.to_radians().cos().max(1e-6), dec_deg)?;
    let north = (pn.0 - p0.0, pn.1 - p0.1);
    let east = (pe.0 - p0.0, pe.1 - p0.1);
    let pa = pa_deg.to_radians();
    let dir = (
        north.0 * pa.cos() + east.0 * pa.sin(),
        north.1 * pa.cos() + east.1 * pa.sin(),
    );
    // Image angles are counter-clockwise while pixel +y points down, hence -dy
    Some((-dir.1).atan2(dir.0).to_degrees())
}

/// Satellite layer. The API is not cfg'd out: without the `satellites` feature the layer
/// reports itself unavailable instead of silently ignoring `satellite_tle`.
#[cfg(feature = "satellites")]
fn satellites_layer(
    wcs: &Wcs,
    tle: &str,
    unix_ms: i64,
    obs: &crate::ephemeris::Observer,
    out: &mut Vec<SatelliteAnnotation>,
    layers: &mut LayerAvailability,
) {
    if let Err(e) = obs.validate() {
        layers.reasons.push(("satellites".into(), e.to_string()));
        return;
    }
    match crate::satellites::satellite_positions(tle, unix_ms, obs) {
        Err(e) => layers.reasons.push(("satellites".into(), e.to_string())),
        Ok(sats) => {
            layers.satellites = true;
            for s in sats {
                if !s.above_horizon {
                    continue; // below the horizon: hidden by the Earth
                }
                let Some((x, y)) = wcs.world_to_pixel(s.ra_deg, s.dec_deg) else {
                    continue;
                };
                if x >= 0.0 && y >= 0.0 && x < wcs.width as f64 && y < wcs.height as f64 {
                    out.push(SatelliteAnnotation {
                        x,
                        y,
                        name: s.name,
                        range_km: s.range_km,
                    });
                }
            }
        }
    }
}

#[cfg(not(feature = "satellites"))]
fn satellites_layer(
    _wcs: &Wcs,
    _tle: &str,
    _unix_ms: i64,
    _obs: &crate::ephemeris::Observer,
    _out: &mut Vec<SatelliteAnnotation>,
    layers: &mut LayerAvailability,
) {
    layers.reasons.push((
        "satellites".into(),
        "engine built without the `satellites` feature".into(),
    ));
}

impl Annotator {
    pub fn annotate(&self, wcs: &Wcs, opts: &AnnotateOptions) -> Annotations {
        let mut layers = LayerAvailability {
            catalog_stars: true,
            ..Default::default()
        };
        // Resolve the language once; fall back to English (and say so) when the pack lacks it
        let lang = self
            .names
            .as_ref()
            .and_then(|n| n.resolve_language(&opts.language));
        if let Some(e) = &self.names_error {
            layers.reasons.push(("names".into(), e.clone()));
        } else if lang.is_none() && !is_chinese(&opts.language) && opts.language != "en" {
            layers.reasons.push((
                "names".into(),
                format!(
                    "language '{}' not in the names pack: using English",
                    opts.language
                ),
            ));
        }
        // Field centre and radius
        let (cra, cdec) = wcs.pixel_to_world(
            (wcs.width as f64 - 1.0) / 2.0,
            (wcs.height as f64 - 1.0) / 2.0,
        );
        let radius_rad = ((wcs.width as f64).hypot(wcs.height as f64) / 2.0
            * wcs.scale_arcsec_per_px()
            / 3600.0
            * 1.15)
            .to_radians();

        // Lines are simplified to half a screen pixel at the app's zoom
        let tol = 0.5
            / opts
                .viewport
                .map(|v| v.scale)
                .filter(|s| s.is_finite() && *s > 0.0)
                .unwrap_or(1.0);
        let view = crate::sky::View::new(wcs, opts.viewport.as_ref());

        // ── Catalog stars ──
        let all_stars = self.db.star_catalog.stars();
        let mut stars: Vec<StarAnnotation> =
            self.db
                .star_catalog
                .query_indices(
                    cra.to_radians() as f32,
                    cdec.to_radians() as f32,
                    radius_rad as f32,
                )
                .into_iter()
                .map(|i| &all_stars[i])
                .filter(|s| opts.star_max_mag.is_none_or(|m| s.mag <= m))
                .filter_map(|s| {
                    let (x, y) = wcs.world_to_pixel(
                        (s.ra_rad as f64).to_degrees(),
                        (s.dec_rad as f64).to_degrees(),
                    )?;
                    (x >= 0.0 && y >= 0.0 && x < wcs.width as f64 && y < wcs.height as f64)
                        .then_some(StarAnnotation {
                            x,
                            y,
                            mag: s.mag,
                            catalog_id: s.id,
                        })
                })
                .collect();
        stars.sort_by(|a, b| a.mag.total_cmp(&b.mag));
        stars.truncate(opts.max_stars);

        // ── IAU named stars (projected from their own table, not matched against the solver catalog) ──
        let mut named_stars = Vec::new();
        if opts.include_star_names {
            layers.named_stars = true;
            for n in crate::names::named_stars() {
                if let Some((x, y)) = wcs.world_to_pixel(n.ra_deg, n.dec_deg) {
                    if x >= 0.0 && y >= 0.0 && x < wcs.width as f64 && y < wcs.height as f64 {
                        named_stars.push(NamedStarAnnotation {
                            x,
                            y,
                            mag: n.mag,
                            hip: n.hip,
                            name: self
                                .localized(&format!("HIP{}", n.hip), lang, Some(n.name_en))
                                .unwrap_or_else(|| n.name_en.to_string()),
                        });
                    }
                }
            }
        }

        // ── Deep-sky objects ──
        let mut objects = Vec::new();
        if opts.include_dso {
            match &self.dso {
                None => {
                    layers.dso = false;
                    layers.reasons.push((
                        "dso".into(),
                        self.dso_error
                            .clone()
                            .unwrap_or_else(|| "no DSO catalog".into()),
                    ));
                }
                Some(cat) => {
                    layers.dso = true;
                    let fov_radius_deg = radius_rad.to_degrees();
                    let scale = wcs.scale_arcsec_per_px();
                    for (i, r) in cat.records().iter().enumerate() {
                        let outlined = opts.dso_outlines && !r.outlines.is_empty();
                        if !outlined
                            && opts
                                .dso_max_mag
                                .is_some_and(|m| r.mag.is_none_or(|v| v > m))
                        {
                            continue;
                        }
                        // Coarse filter: angular distance from the field centre (planar approximation)
                        let dra = {
                            let d = (r.ra_deg - cra).rem_euclid(360.0);
                            d.min(360.0 - d)
                        };
                        let ang = ((r.dec_deg - cdec).powi(2)
                            + (dra * cdec.to_radians().cos()).powi(2))
                        .sqrt();
                        let extent_deg =
                            (r.major_arcmin.unwrap_or(0.0) as f64 / 60.0).max(if outlined {
                                cat.outline_radius_deg(i)
                            } else {
                                0.0
                            });
                        if ang > fov_radius_deg + extent_deg {
                            continue;
                        }
                        let Some((x, y)) = wcs.world_to_pixel(r.ra_deg, r.dec_deg) else {
                            continue;
                        };
                        let semi_major_px =
                            r.major_arcmin.unwrap_or(0.0) as f64 * 60.0 / 2.0 / scale;
                        let semi_minor_px = r
                            .minor_arcmin
                            .map(|m| m as f64 * 60.0 / 2.0 / scale)
                            .unwrap_or(semi_major_px);
                        let outlines = if outlined {
                            project_outlines(wcs, r, opts.max_outline_level, tol)
                        } else {
                            Vec::new()
                        };
                        let margin = semi_major_px.max(1.0);
                        if outlines.is_empty()
                            && (x < -margin
                                || y < -margin
                                || x >= wcs.width as f64 + margin
                                || y >= wcs.height as f64 + margin)
                        {
                            continue;
                        }
                        // Asymmetric size but unknown PA: never guess the orientation (angle=None, draw a circle)
                        let symmetric =
                            r.minor_arcmin.is_none() || r.minor_arcmin == r.major_arcmin;
                        let angle_deg = match (r.pa_deg, symmetric) {
                            (Some(pa), _) => pa_to_image_angle(wcs, r.ra_deg, r.dec_deg, pa as f64),
                            (None, true) => Some(0.0),
                            (None, false) => None,
                        };
                        objects.push(DsoAnnotation {
                            x,
                            y,
                            semi_major_px,
                            semi_minor_px,
                            angle_deg,
                            designation: r.designation.clone(),
                            // Names pack first; for Chinese, fall back to the catalog's curated
                            // Chinese name, then the English name, then the designation
                            common_name: self.localized(
                                &r.designation,
                                lang,
                                if is_chinese(&opts.language) {
                                    r.common_name_zh.as_deref().or(r.common_name_en.as_deref())
                                } else {
                                    r.common_name_en.as_deref()
                                },
                            ),
                            kind: r.kind,
                            mag: r.mag,
                            outlines,
                        });
                    }
                    objects.sort_by(|a, b| b.semi_major_px.total_cmp(&a.semi_major_px));
                }
            }
        }

        // ── Solar system (needs the observation time; degrades openly without it) ──
        let mut solar = Vec::new();
        if opts.include_solar_system {
            match opts.observation_unix_ms {
                None => {
                    layers.solar_system = false;
                    layers.reasons.push((
                        "solar_system".into(),
                        "solar-system positions require an observation time".into(),
                    ));
                }
                Some(ms) => {
                    layers.solar_system = true;
                    if opts.observer.is_none() {
                        // Say why the moon may be off by up to 1°
                        layers.reasons.push((
                            "solar_system".into(),
                            "geocentric positions: pass an observer for the moon's topocentric parallax (up to 1 deg)".into(),
                        ));
                    }
                    let scale = wcs.scale_arcsec_per_px();
                    for b in crate::ephemeris::solar_system_positions_at(ms, opts.observer.as_ref())
                    {
                        if let Some((x, y)) = wcs.world_to_pixel(b.ra_deg, b.dec_deg) {
                            let margin = b
                                .angular_radius_deg
                                .map(|r| r * 3600.0 / scale)
                                .unwrap_or(0.0)
                                .max(1.0);
                            if x < -margin
                                || y < -margin
                                || x >= wcs.width as f64 + margin
                                || y >= wcs.height as f64 + margin
                            {
                                continue;
                            }
                            solar.push(SolarAnnotation {
                                x,
                                y,
                                name: self
                                    .localized(
                                        b.name_en,
                                        lang,
                                        Some(if is_chinese(&opts.language) {
                                            b.name_zh
                                        } else {
                                            b.name_en
                                        }),
                                    )
                                    .unwrap_or_else(|| b.name_en.to_string()),
                                angular_radius_px: b.angular_radius_deg.map(|r| r * 3600.0 / scale),
                            });
                        }
                    }
                }
            }
        }

        // ── Satellite passes (TLEs supplied by the caller; the engine never goes online) ──
        let mut satellites = Vec::new();
        if let Some(tle) = opts.satellite_tle.as_deref() {
            match (opts.observation_unix_ms, opts.observer) {
                (Some(ms), Some(obs)) => {
                    satellites_layer(wcs, tle, ms, &obs, &mut satellites, &mut layers)
                }
                _ => layers.reasons.push((
                    "satellites".into(),
                    "satellite layer needs both an observation time and an observer".into(),
                )),
            }
        }

        // ── Constellations: figures with names, and IAU boundaries ──
        let (mut constellations, mut boundaries) = (Vec::new(), Vec::new());
        if opts.include_constellations || opts.constellation_boundaries {
            match &self.constellations {
                None => layers
                    .reasons
                    .push(("constellations".into(), self.constellations_error.clone())),
                // A viewport off the image shows nothing
                Some(loaded) => {
                    if let Some(view) = &view {
                        layers.constellations = true;
                        let pack = &loaded.pack;
                        if opts.include_constellations {
                            for (i, c) in pack.constellations.iter().enumerate() {
                                let lines = loaded.figure(view, i);
                                if lines.is_empty() {
                                    continue;
                                }
                                let label = view.visible_point(c.label).or_else(|| {
                                    let inside: Vec<[f64; 2]> = lines
                                        .iter()
                                        .flatten()
                                        .copied()
                                        .filter(|&p| view.contains(p))
                                        .collect();
                                    (!inside.is_empty()).then(|| {
                                        let n = inside.len() as f64;
                                        [
                                            inside.iter().map(|p| p[0]).sum::<f64>() / n,
                                            inside.iter().map(|p| p[1]).sum::<f64>() / n,
                                        ]
                                    })
                                });
                                constellations.push(
                                    crate::constellations::ConstellationAnnotation {
                                        abbr: c.abbr.clone(),
                                        name: self
                                            .localized(
                                                &format!("CON {}", c.abbr),
                                                lang,
                                                Some(&c.name),
                                            )
                                            .unwrap_or_else(|| c.name.clone()),
                                        label,
                                        lines,
                                    },
                                );
                            }
                        }
                        if opts.constellation_boundaries {
                            for (i, points) in loaded.boundaries(view) {
                                boundaries.push(crate::constellations::BoundaryAnnotation {
                                    between: pack.boundaries[i]
                                        .between
                                        .map(|k| pack.constellations[k as usize].abbr.clone()),
                                    points,
                                });
                            }
                        }
                    }
                }
            }
        }

        // ── Coordinate grids ──
        let mut grid = Vec::new();
        if let Some(view) = &view {
            let spacing = opts
                .grid_spacing_px
                .filter(|s| s.is_finite() && *s >= 10.0)
                .unwrap_or(crate::grid::DEFAULT_SPACING_PX);
            if opts.equatorial_grid {
                layers.grid = true;
                grid.extend(crate::grid::equatorial(view, spacing));
            }
            if opts.horizontal_grid {
                match (opts.observation_unix_ms, opts.observer) {
                    (Some(ms), Some(obs)) => {
                        layers.grid = true;
                        grid.extend(crate::grid::horizontal(view, spacing, ms, &obs));
                    }
                    _ => layers.reasons.push((
                        "grid".into(),
                        "horizontal grid needs an observation time and an observer".into(),
                    )),
                }
            }
        }

        Annotations {
            stars,
            named_stars,
            objects,
            solar,
            satellites,
            constellations,
            boundaries,
            grid,
            layers,
        }
    }
}
