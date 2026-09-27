use unisolver_core::*;
use unisolver_synth as synth;

/// Bundled multilingual names pack (assertions that need it are skipped when it is absent)
fn names_pack_path() -> Option<String> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/unisolver_flutter/lib/optional/unisolver_names.bin");
    p.exists().then(|| p.to_string_lossy().to_string())
}

fn test_db_path() -> String {
    synth::test_db_file("unisolver_core_test.db")
}

#[test]
fn t2_catalog_stars_project_into_field() {
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let q = synth::look_at(120.0, 40.0, 15.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        5,
    );
    let out = solver
        .solve(
            &Frame {
                width: 1024,
                height: 768,
                row_stride_bytes: None,
                pixels: PixelData::LumaF32(img),
            },
            &SolveOptions::new(20.0),
        )
        .unwrap();
    let g = out.solution.unwrap();

    let ann = solver.annotator(None, None).unwrap();
    let a = ann.annotate(
        &g.wcs,
        &AnnotateOptions {
            star_max_mag: Some(7.0),
            ..Default::default()
        },
    );
    assert!(a.layers.catalog_stars);
    assert!(!a.layers.dso, "no dso file given");
    assert!(a.stars.len() >= 10, "got {}", a.stars.len());
    for s in &a.stars {
        assert!(s.x >= 0.0 && s.x < 1024.0 && s.y >= 0.0 && s.y < 768.0);
        assert!(s.mag <= 7.0);
    }
    // Aligned with matched stars: each matched catalog_id's marker lies within 1.5 px of its centroid
    let mut checked = 0;
    for m in g.matched.iter() {
        if let Some(s) = a.stars.iter().find(|s| s.catalog_id == m.catalog_id) {
            let d = (s.x - m.x).hypot(s.y - m.y);
            assert!(d < 1.5, "id {} off by {d}", m.catalog_id);
            checked += 1;
        }
    }
    assert!(checked >= 5, "only {checked} matched stars verified");
}

#[test]
fn t25_vega_named_in_field() {
    let solver = Solver::from_file(&test_db_path()).unwrap();
    // The solver database is a synthetic sky; the named-star layer is independent (own
    // projection), so the WCS is built by hand, pointing at Vega
    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [279.2347, 38.7837],
        theta_rad: 0.0,
        camera: cam,
    };
    // Localized names come from the names pack; without it only English (checked below)
    let ann = solver
        .annotator(None, names_pack_path().as_deref())
        .unwrap();
    let opts = AnnotateOptions {
        language: "zh_cn".into(),
        ..Default::default()
    };
    let a = ann.annotate(&wcs, &opts);
    assert!(a.layers.named_stars);
    let vega = a
        .named_stars
        .iter()
        .find(|n| n.hip == 91262)
        .expect("vega present");
    if names_pack_path().is_some() {
        assert!(vega.name.contains("织女一"), "zh name was {:?}", vega.name);
    } else {
        assert_eq!(vega.name, "Vega");
    }
    // The field is centred on Vega, so its marker is near the centre
    assert!(
        (vega.x - 511.5).abs() < 3.0 && (vega.y - 383.5).abs() < 3.0,
        "vega at ({}, {})",
        vega.x,
        vega.y
    );
    let en = AnnotateOptions {
        language: "en".into(),
        ..Default::default()
    };
    let b = ann.annotate(&wcs, &en);
    assert_eq!(
        b.named_stars.iter().find(|n| n.hip == 91262).unwrap().name,
        "Vega"
    );
}

#[test]
fn t3_dso_projection_and_pa_consistency() {
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let dso_path = std::env::temp_dir().join("udso_m42.bin");
    unisolver_core::dso::write_catalog(
        dso_path.to_str().unwrap(),
        &[unisolver_core::dso::DsoRecord {
            designation: "M42".into(),
            common_name_en: Some("Orion Nebula".into()),
            common_name_zh: Some("猎户座大星云".into()),
            kind: unisolver_core::dso::DsoKind::Nebula,
            ra_deg: 83.822,
            dec_deg: -5.391,
            mag: Some(4.0),
            major_arcmin: Some(85.0),
            minor_arcmin: Some(60.0),
            pa_deg: Some(0.0),
            outlines: Vec::new(),
        }],
    )
    .unwrap();
    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [83.822, -5.391],
        theta_rad: 0.3,
        camera: cam,
    };
    let ann = solver
        .annotator(Some(dso_path.to_str().unwrap()), None)
        .unwrap();
    let opts = AnnotateOptions {
        language: "zh_cn".into(),
        ..Default::default()
    };
    let a = ann.annotate(&wcs, &opts);
    assert!(a.layers.dso);
    let m42 = a
        .objects
        .iter()
        .find(|o| o.designation == "M42")
        .expect("m42");
    assert_eq!(m42.common_name.as_deref(), Some("猎户座大星云"));
    assert!(
        (m42.x - 511.5).abs() < 2.0 && (m42.y - 383.5).abs() < 2.0,
        "m42 at ({}, {})",
        m42.x,
        m42.y
    );
    assert!(m42.semi_major_px > 10.0);
    // PA consistency: the angle matches the direction of a small northward offset, projected (±1°)
    let p0 = wcs.world_to_pixel(83.822, -5.391).unwrap();
    let pn = wcs.world_to_pixel(83.822, -5.341).unwrap();
    let measured = (-(pn.1 - p0.1)).atan2(pn.0 - p0.0).to_degrees();
    let diff = (m42.angle_deg.unwrap() - measured + 540.0).rem_euclid(360.0) - 180.0;
    assert!(
        diff.abs() < 1.0,
        "angle {} vs measured {}",
        m42.angle_deg.unwrap(),
        measured
    );
}

#[test]
fn dso_layer_reports_unavailable_on_bad_path() {
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let ann = solver.annotator(Some("/nonexistent/x.bin"), None).unwrap();
    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [10.0, 10.0],
        theta_rad: 0.0,
        camera: cam,
    };
    let a = ann.annotate(&wcs, &AnnotateOptions::default());
    assert!(!a.layers.dso);
    assert!(a.layers.reasons.iter().any(|(k, _)| k == "dso"));
}

#[test]
fn t4_solar_system_layer_with_and_without_time() {
    let solver = Solver::from_file(&test_db_path()).unwrap();
    // 2026-08-31T00:00Z Jupiter is at (135.73, +17.38) per astropy; the field points at it
    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [135.73, 17.38],
        theta_rad: 0.0,
        camera: cam,
    };
    let ann = solver.annotator(None, None).unwrap();
    let mut opts = AnnotateOptions {
        language: "zh_cn".into(),
        observation_unix_ms: Some(1_788_134_400_000),
        ..Default::default()
    };
    let a = ann.annotate(&wcs, &opts);
    assert!(a.layers.solar_system);
    let jup = a
        .solar
        .iter()
        .find(|s| s.name == "木星")
        .expect("jupiter in field");
    assert!(
        (jup.x - 511.5).abs() < 30.0 && (jup.y - 383.5).abs() < 30.0,
        "jupiter at ({}, {})",
        jup.x,
        jup.y
    );
    // No time → the layer is unavailable, with a reason
    opts.observation_unix_ms = None;
    let b = ann.annotate(&wcs, &opts);
    assert!(!b.layers.solar_system);
    assert!(b.layers.reasons.iter().any(|(k, _)| k == "solar_system"));
    assert!(b.solar.is_empty());
}

/// Moon parallax in the annotation layer: the field points at astropy's **topocentric** moon.
/// With an observer the moon lands at the centre; without one it is off by about half
/// the field (0.96° ≈ 49 px).
#[test]
fn t4_moon_uses_the_observer_for_parallax() {
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let ann = solver.annotator(None, None).unwrap();
    // 2026-08-31T00:00Z, Shanghai; astropy topocentric moon (8.3529, 7.3103)
    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [8.3529, 7.3103],
        theta_rad: 0.0,
        camera: cam,
    };
    let (cx, cy) = (511.5, 383.5);
    let base = AnnotateOptions {
        language: "zh_cn".into(),
        observation_unix_ms: Some(1_788_134_400_000),
        observer: Some(Observer {
            lat_deg: 31.2,
            lon_deg: 121.5,
            alt_m: 10.0,
        }),
        ..Default::default()
    };
    let topo = ann.annotate(&wcs, &base);
    let m = topo
        .solar
        .iter()
        .find(|s| s.name == "月亮")
        .expect("moon in field");
    let d_topo = ((m.x - cx).powi(2) + (m.y - cy).powi(2)).sqrt();
    // The 0.1° criterion in pixels: 20°/1024 px ≈ 0.0195°/px → 5 px
    assert!(d_topo < 5.0, "topocentric moon {d_topo:.1} px off centre");
    // The apparent radius must be present too (the moon is extended; draw a circle)
    assert!(m.angular_radius_px.unwrap() > 5.0);

    let geo = ann.annotate(
        &wcs,
        &AnnotateOptions {
            observer: None,
            ..base.clone()
        },
    );
    let g = geo.solar.iter().find(|s| s.name == "月亮").unwrap();
    let d_geo = ((g.x - cx).powi(2) + (g.y - cy).powi(2)).sqrt();
    assert!(
        d_geo > 30.0,
        "geocentric moon should be ~49px off, got {d_geo:.1}"
    );
    // The degradation must be stated (so a UI can suggest "give a location for accuracy")
    assert!(geo
        .layers
        .reasons
        .iter()
        .any(|(k, v)| k == "solar_system" && v.contains("topocentric")));
}

/// Satellite layer: TLEs from the caller, positions projected to pixels, nothing below the horizon.
#[cfg(feature = "satellites")]
#[test]
fn t5_satellite_layer_projects_and_degrades_honestly() {
    const ISS: &str = "ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000-0  30777-3 0  9991
2 25544  51.6400 208.9163 0006317  69.9862 290.2117 15.49560538429085";
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let ann = solver.annotator(None, None).unwrap();
    let obs = Observer {
        lat_deg: 31.2,
        lon_deg: 121.5,
        alt_m: 10.0,
    };
    // Find a time when the ISS is above Shanghai's horizon. One orbit is ~93 min but only
    // some passes cross a given site, so scan 24 hours by the minute (SGP4 within a day of
    // epoch is accurate enough for these assertions)
    let t0 = 1_704_110_400_000i64;
    let (ms, sat) = (0..1440)
        .filter_map(|k| {
            let ms = t0 + k * 60_000;
            let s = unisolver_core::satellites::satellite_positions(ISS, ms, &obs)
                .unwrap()
                .pop()?;
            s.above_horizon.then_some((ms, s))
        })
        .next()
        .expect("ISS rises within one orbit");

    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [sat.ra_deg, sat.dec_deg],
        theta_rad: 0.0,
        camera: cam,
    };
    let opts = AnnotateOptions {
        observation_unix_ms: Some(ms),
        observer: Some(obs),
        satellite_tle: Some(ISS.to_string()),
        ..Default::default()
    };
    let a = ann.annotate(&wcs, &opts);
    assert!(a.layers.satellites, "reasons: {:?}", a.layers.reasons);
    let s = a.satellites.first().expect("ISS in field");
    assert_eq!(s.name, "ISS (ZARYA)");
    assert!(((s.x - 511.5).powi(2) + (s.y - 383.5).powi(2)).sqrt() < 5.0);
    assert!(
        s.range_km > 350.0 && s.range_km < 2_500.0,
        "range {}",
        s.range_km
    );

    // Missing observer or time → the layer is unavailable with a reason (not a silent empty layer)
    for (t, o) in [(Some(ms), None), (None, Some(obs))] {
        let b = ann.annotate(
            &wcs,
            &AnnotateOptions {
                observation_unix_ms: t,
                observer: o,
                satellite_tle: Some(ISS.to_string()),
                ..Default::default()
            },
        );
        assert!(!b.layers.satellites && b.satellites.is_empty());
        assert!(b.layers.reasons.iter().any(|(k, _)| k == "satellites"));
    }

    // Garbage TLE: layer unavailable with a reason; never a panic, never "zero passes"
    let c = ann.annotate(
        &wcs,
        &AnnotateOptions {
            observation_unix_ms: Some(ms),
            observer: Some(obs),
            satellite_tle: Some("<html>404 not found</html>".into()),
            ..Default::default()
        },
    );
    assert!(!c.layers.satellites);
    assert!(c
        .layers
        .reasons
        .iter()
        .any(|(k, v)| k == "satellites" && v.contains("TLE")));
}

/// DSO name fallback: Chinese when available, else English, else the designation.
/// (English mode **never falls back to Chinese**: a Chinese name in an English UI is a bug.)
#[test]
fn t3_dso_name_falls_back_zh_to_en_to_designation() {
    use unisolver_core::dso::{DsoKind, DsoRecord};
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let path = std::env::temp_dir().join("udso_fallback.bin");
    let rec = |desig: &str, en: Option<&str>, zh: Option<&str>, dec: f64| DsoRecord {
        designation: desig.into(),
        common_name_en: en.map(Into::into),
        common_name_zh: zh.map(Into::into),
        kind: DsoKind::Galaxy,
        ra_deg: 10.0,
        dec_deg: dec,
        mag: Some(8.0),
        major_arcmin: Some(5.0),
        minor_arcmin: Some(5.0),
        pa_deg: None,
        outlines: Vec::new(),
    };
    unisolver_core::dso::write_catalog(
        path.to_str().unwrap(),
        &[
            rec("M31", Some("Andromeda Galaxy"), Some("仙女座星系"), 10.0),
            rec("NGC0891", Some("Silver Sliver"), None, 10.5),
            rec("IC4628", None, None, 9.5),
        ],
    )
    .unwrap();
    let ann = solver
        .annotator(path.to_str().unwrap().into(), None)
        .unwrap();
    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [10.0, 10.0],
        theta_rad: 0.0,
        camera: cam,
    };
    let name_of = |a: &Annotations, d: &str| {
        a.objects
            .iter()
            .find(|o| o.designation == d)
            .unwrap_or_else(|| panic!("{d} missing"))
            .common_name
            .clone()
    };
    let zh = ann.annotate(
        &wcs,
        &AnnotateOptions {
            language: "zh_cn".into(),
            ..Default::default()
        },
    );
    assert_eq!(name_of(&zh, "M31").as_deref(), Some("仙女座星系"));
    assert_eq!(name_of(&zh, "NGC0891").as_deref(), Some("Silver Sliver"));
    assert_eq!(name_of(&zh, "IC4628"), None);

    let en = ann.annotate(&wcs, &AnnotateOptions::default());
    assert_eq!(name_of(&en, "M31").as_deref(), Some("Andromeda Galaxy"));
    assert_eq!(name_of(&en, "NGC0891").as_deref(), Some("Silver Sliver"));
    assert_eq!(name_of(&en, "IC4628"), None);
}

/// The bundled DSO catalog really carries its Chinese names (skipped when absent).
/// This locks the **data**; the previous test locks the fallback logic.
#[test]
fn packaged_dso_catalog_carries_chinese_names() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("packages/unisolver_flutter/assets/unisolver_dso.bin");
    if !path.exists() {
        eprintln!("skipped: packaged DSO catalog missing");
        return;
    }
    let cat = unisolver_core::dso::DsoCatalog::open(path.to_str().unwrap()).unwrap();
    let find = |d: &str| {
        cat.records()
            .iter()
            .find(|r| r.designation == d)
            .unwrap_or_else(|| panic!("{d} not in catalog"))
    };
    assert_eq!(find("M31").common_name_zh.as_deref(), Some("仙女座星系"));
    assert_eq!(find("M42").common_name_zh.as_deref(), Some("猎户座大星云"));
    // M45 has no NGC number and is missing from OpenNGC's main table: dsogen's addendum supplies it
    let m45 = find("M45");
    assert_eq!(m45.common_name_zh.as_deref(), Some("昴星团"));
    assert!(
        m45.major_arcmin.unwrap() > 60.0,
        "the Pleiades span about 110′"
    );
    let zh = cat
        .records()
        .iter()
        .filter(|r| r.common_name_zh.is_some())
        .count();
    assert!(zh >= 100, "only {zh} Chinese names");
}

/// Multilingual: the same frame with another language code changes the names; a language
/// missing from the pack falls back to English.
#[test]
fn names_pack_localizes_stars_dso_and_planets() {
    let Some(pack) = names_pack_path() else {
        eprintln!("skipped: names pack missing");
        return;
    };
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let dso = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/unisolver_flutter/assets/unisolver_dso.bin");
    let ann = solver
        .annotator(dso.to_str().filter(|_| dso.exists()), Some(&pack))
        .unwrap();
    assert!(
        ann.languages().len() >= 10,
        "languages: {:?}",
        ann.languages()
    );

    // The field points at Vega; change the language and read the same star's name
    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [279.2347, 38.7837],
        theta_rad: 0.0,
        camera: cam,
    };
    let name_in = |lang: &str| {
        ann.annotate(
            &wcs,
            &AnnotateOptions {
                language: lang.into(),
                ..Default::default()
            },
        )
        .named_stars
        .iter()
        .find(|n| n.hip == 91262)
        .map(|n| n.name.clone())
        .expect("vega")
    };
    assert!(name_in("zh_cn").contains("织女一"));
    assert!(
        name_in("zh-CN").contains("织女一"),
        "language codes are matched leniently"
    );
    assert!(name_in("zh").contains("织女一"));
    assert_eq!(name_in("en"), "Vega");
    let ja = name_in("ja");
    assert!(ja != "Vega" && !ja.is_empty(), "Japanese name: {ja}");
    // A language missing from the pack → English, and say so
    let sw = ann.annotate(
        &wcs,
        &AnnotateOptions {
            language: "sw".into(),
            ..Default::default()
        },
    );
    assert_eq!(
        sw.named_stars.iter().find(|n| n.hip == 91262).unwrap().name,
        "Vega"
    );
    assert!(sw.layers.reasons.iter().any(|(k, _)| k == "names"));
}

/// Solar-system names also come from the names pack (the source data has only deep-sky
/// objects and stars; namesgen adds the planets from a built-in table).
#[test]
fn names_pack_localizes_solar_system() {
    let Some(pack) = names_pack_path() else {
        eprintln!("skipped: names pack missing");
        return;
    };
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let ann = solver.annotator(None, Some(&pack)).unwrap();
    let cam = CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap();
    // 2026-08-31 Jupiter is at (135.73, +17.38)
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [135.73, 17.38],
        theta_rad: 0.0,
        camera: cam,
    };
    let jup = |lang: &str| {
        ann.annotate(
            &wcs,
            &AnnotateOptions {
                language: lang.into(),
                observation_unix_ms: Some(1_788_134_400_000),
                ..Default::default()
            },
        )
        .solar
        .iter()
        .map(|s| s.name.clone())
        .find(|n| !n.is_empty())
    };
    assert_eq!(jup("zh_cn").as_deref(), Some("木星"));
    assert_eq!(jup("en").as_deref(), Some("Jupiter"));
    assert_eq!(jup("ja").as_deref(), Some("木星"));
    assert_eq!(jup("fr").as_deref(), Some("Jupiter"));
    assert_eq!(jup("ru").as_deref(), Some("Юпитер"));
}

/// Outlines project vertex by vertex; a contour behind the camera is dropped; outlined
/// objects pass the magnitude filter (extended nebulae rarely have one); off switches them off.
#[test]
fn dso_outlines_project_to_pixels() {
    use unisolver_core::dso::{DsoKind, DsoRecord, OutlineLevel, OutlineRing};
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let dso_path = std::env::temp_dir().join("udso_outline.bin");
    let square = vec![(82.0, -4.0), (85.5, -4.0), (85.5, -7.0), (82.0, -7.0)];
    let behind = vec![(263.8, 5.4), (264.0, 5.4), (264.0, 5.2)]; // the opposite sky
    unisolver_core::dso::write_catalog(
        dso_path.to_str().unwrap(),
        &[DsoRecord {
            designation: "NGC1976".into(),
            common_name_en: Some("Orion Nebula".into()),
            common_name_zh: None,
            kind: DsoKind::Nebula,
            ra_deg: 83.822,
            dec_deg: -5.391,
            mag: None,
            major_arcmin: Some(85.0),
            minor_arcmin: Some(60.0),
            pa_deg: None,
            outlines: vec![
                OutlineLevel {
                    level: 1,
                    contours: vec![
                        OutlineRing {
                            closed: true,
                            vertices: square.clone(),
                        },
                        OutlineRing {
                            closed: true,
                            vertices: behind,
                        },
                    ],
                },
                OutlineLevel {
                    level: 3,
                    contours: vec![OutlineRing {
                        closed: true,
                        vertices: vec![(83.7, -5.3), (83.9, -5.3), (83.8, -5.5)],
                    }],
                },
            ],
        }],
    )
    .unwrap();
    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [83.822, -5.391],
        theta_rad: 0.3,
        camera: CameraParams::from_horizontal_fov(20.0, 1024, 768).unwrap(),
    };
    let ann = solver
        .annotator(Some(dso_path.to_str().unwrap()), None)
        .unwrap();
    let opts = AnnotateOptions {
        dso_max_mag: Some(6.0),
        ..Default::default()
    };
    let a = ann.annotate(&wcs, &opts);
    let neb = a.objects.iter().find(|o| o.designation == "NGC1976");
    let neb = neb.expect("an outlined object passes the magnitude filter");
    assert_eq!(neb.outlines.len(), 2);
    let lv1 = &neb.outlines[0];
    assert_eq!(lv1.level, 1);
    assert_eq!(
        lv1.contours.len(),
        1,
        "the contour behind the camera is dropped"
    );
    let c = &lv1.contours[0];
    assert!(c.closed);
    assert_eq!(c.points.len(), 4);
    for (p, (ra, dec)) in c.points.iter().zip(&square) {
        let want = wcs.world_to_pixel(*ra as f64, *dec as f64).unwrap();
        assert!(
            (p[0] - want.0).abs() < 1e-6 && (p[1] - want.1).abs() < 1e-6,
            "{p:?} vs {want:?}"
        );
    }

    let only_outer = AnnotateOptions {
        max_outline_level: 1,
        ..Default::default()
    };
    let a = ann.annotate(&wcs, &only_outer);
    assert_eq!(a.objects[0].outlines.len(), 1);
    let off = AnnotateOptions {
        dso_outlines: false,
        ..Default::default()
    };
    assert!(ann.annotate(&wcs, &off).objects[0].outlines.is_empty());
}

/// Constellation figures and boundaries: projected where the camera looks, clipped where it
/// does not, localized through the names pack, and silent unless asked for.
#[test]
fn constellation_layers_project_figures_boundaries_and_names() {
    use unisolver_core::constellations::{BoundaryEdge, ConstellationFigure, ConstellationPack};
    use unisolver_core::names_pack::NamesPack;
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let dir = std::env::temp_dir().join(format!("ucon_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (betelgeuse, bellatrix, rigel) = ([88.79f32, 7.41], [81.28f32, 6.35], [78.63f32, -8.20]);
    let pack = ConstellationPack {
        constellations: vec![
            ConstellationFigure {
                abbr: "Ori".into(),
                name: "Orion".into(),
                // A figure in the frame, and a long line leaving the camera's view
                lines: vec![
                    vec![betelgeuse, bellatrix, rigel],
                    vec![[83.0, 0.0], [150.0, 0.0]],
                ],
                label: [83.0, 1.0],
            },
            ConstellationFigure {
                abbr: "Tau".into(),
                name: "Taurus".into(),
                lines: vec![vec![[300.0, 40.0], [310.0, 45.0]]],
                label: [305.0, 42.0],
            },
        ],
        boundaries: vec![BoundaryEdge {
            between: [0, 1],
            points: (0..=12).map(|k| [86.0, 22.0 - k as f32]).collect(),
        }],
    };
    let pack_path = dir.join("unisolver_constellations.bin");
    pack.write(pack_path.to_str().unwrap()).unwrap();
    let names_path = dir.join("names.bin");
    NamesPack {
        languages: vec!["en".into(), "zh_cn".into()],
        entries: [(
            "CON Ori".to_string(),
            vec![Some("Orion".into()), Some("猎户座".into())],
        )]
        .into(),
    }
    .write(names_path.to_str().unwrap())
    .unwrap();

    let wcs = Wcs {
        width: 1024,
        height: 768,
        cd: [[0.0; 2]; 2],
        crval_deg: [83.8, -1.0],
        theta_rad: 0.2,
        camera: CameraParams::from_horizontal_fov(30.0, 1024, 768).unwrap(),
    };
    let both = AnnotateOptions {
        include_constellations: true,
        constellation_boundaries: true,
        language: "zh_cn".into(),
        ..Default::default()
    };

    // Not asked for: nothing, and no complaint
    let plain = solver.annotator(None, None).unwrap();
    let a = plain.annotate(&wcs, &AnnotateOptions::default());
    assert!(a.constellations.is_empty() && a.boundaries.is_empty());
    assert!(!a.layers.reasons.iter().any(|(l, _)| l == "constellations"));
    // Asked for without a pack: unavailable, with the reason
    let a = plain.annotate(&wcs, &both);
    assert!(!a.layers.constellations);
    assert!(a
        .layers
        .reasons
        .iter()
        .any(|(l, r)| l == "constellations" && r.contains("no constellation pack")));
    let broken = solver
        .annotator(None, None)
        .unwrap()
        .with_constellations(Some("/nonexistent/ucon.bin"));
    assert!(broken
        .annotate(&wcs, &both)
        .layers
        .reasons
        .iter()
        .any(|(l, _)| l == "constellations"));

    let ann = solver
        .annotator(None, Some(names_path.to_str().unwrap()))
        .unwrap()
        .with_constellations(Some(pack_path.to_str().unwrap()));
    let a = ann.annotate(&wcs, &both);
    assert!(a.layers.constellations);
    assert_eq!(
        a.constellations.len(),
        1,
        "Taurus's figure is on the other side of the sky"
    );
    let ori = &a.constellations[0];
    assert_eq!((ori.abbr.as_str(), ori.name.as_str()), ("Ori", "猎户座"));
    let px = |v: [f32; 2]| wcs.world_to_pixel(v[0] as f64, v[1] as f64).unwrap();
    let label = ori.label.expect("the anchor is in the frame");
    let want = px([83.0, 1.0]);
    assert!((label[0] - want.0).hypot(label[1] - want.1) < 1e-6);
    // Stars land where the WCS puts them, and an ideal camera maps great-circle arcs to
    // straight lines, so the sampled figure simplifies back to its three stars
    let figure = ori
        .lines
        .iter()
        .find(|l| l.len() == 3)
        .unwrap_or_else(|| panic!("{:?}", ori.lines));
    for (p, star) in figure.iter().zip([betelgeuse, bellatrix, rigel]) {
        let s = px(star);
        assert!((p[0] - s.0).hypot(p[1] - s.1) < 1e-6, "{p:?} vs {s:?}");
    }
    // The long line runs from the centre past the frame edge, and stops where the view ends
    let long = ori
        .lines
        .iter()
        .find(|l| l.len() != 3)
        .expect("the long line");
    let far = long.iter().map(|p| p[0]).fold(f64::MIN, f64::max);
    assert!(far > 1024.0 && far < 1024.0 * 3.0, "{far}");

    assert_eq!(a.boundaries.len(), 1);
    let b = &a.boundaries[0];
    assert_eq!(b.between, ["Ori".to_string(), "Tau".to_string()]);
    assert!(b
        .points
        .iter()
        .any(|p| p[0] >= 0.0 && p[0] < 1024.0 && p[1] >= 0.0 && p[1] < 768.0));

    // Without the names pack: the IAU name
    let latin = solver
        .annotator(None, None)
        .unwrap()
        .with_constellations(Some(pack_path.to_str().unwrap()));
    assert_eq!(latin.annotate(&wcs, &both).constellations[0].name, "Orion");
    let _ = std::fs::remove_dir_all(dir);
}

/// The bundled constellation pack: all 88 constellations, every one bordered, and real
/// fields show the constellations they should. Skipped when the pack is absent.
#[test]
fn bundled_constellation_pack_draws_real_skies() {
    use unisolver_core::constellations::ConstellationPack;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/unisolver_flutter/assets/unisolver_constellations.bin");
    if !path.exists() {
        eprintln!("skipped: constellation pack not generated");
        return;
    }
    let pack = ConstellationPack::open(path.to_str().unwrap()).unwrap();
    assert_eq!(pack.constellations.len(), 88);
    let mut bordered = [false; 88];
    for b in &pack.boundaries {
        for i in b.between {
            bordered[i as usize] = true;
        }
        // Densified: neighbouring points no more than a fraction of a degree apart
        for w in b.points.windows(2) {
            let d = ((w[1][0] - w[0][0])
                .rem_euclid(360.0)
                .min((w[0][0] - w[1][0]).rem_euclid(360.0))
                * w[0][1].to_radians().cos())
            .hypot(w[1][1] - w[0][1]);
            assert!(d < 0.25, "{d}");
        }
    }
    assert!(
        bordered.iter().all(|&b| b),
        "every constellation has a boundary"
    );
    // Orion's figure passes through Betelgeuse and Rigel (J2000)
    let ori = pack
        .constellations
        .iter()
        .find(|c| c.abbr == "Ori")
        .unwrap();
    let near = |ra: f32, dec: f32| {
        ori.lines
            .iter()
            .flatten()
            .any(|v| (v[0] - ra).abs() < 0.01 && (v[1] - dec).abs() < 0.01)
    };
    assert!(
        near(88.793, 7.407) && near(78.634, -8.202),
        "{:?}",
        ori.lines
    );

    // A 70° field on Orion: Orion and its neighbours, with boundaries between them
    let solver = Solver::from_file(&test_db_path()).unwrap();
    let ann = solver
        .annotator(None, names_pack_path().as_deref())
        .unwrap()
        .with_constellations(path.to_str());
    let wcs = Wcs {
        width: 1920,
        height: 1080,
        cd: [[0.0; 2]; 2],
        crval_deg: [83.8, 0.0],
        theta_rad: 0.0,
        camera: CameraParams::from_horizontal_fov(70.0, 1920, 1080).unwrap(),
    };
    let a = ann.annotate(
        &wcs,
        &AnnotateOptions {
            include_constellations: true,
            constellation_boundaries: true,
            language: "zh_cn".into(),
            ..Default::default()
        },
    );
    let abbrs: Vec<&str> = a.constellations.iter().map(|c| c.abbr.as_str()).collect();
    for want in ["Ori", "Tau", "Mon", "CMi", "Lep", "Eri"] {
        assert!(abbrs.contains(&want), "{want} missing from {abbrs:?}");
    }
    assert!(a
        .boundaries
        .iter()
        .any(|b| b.between.contains(&"Ori".to_string())));
    if names_pack_path().is_some() {
        let ori = a.constellations.iter().find(|c| c.abbr == "Ori").unwrap();
        assert_eq!(ori.name, "猎户座");
    }
    let label = a
        .constellations
        .iter()
        .find(|c| c.abbr == "Ori")
        .unwrap()
        .label
        .unwrap();
    assert!(label[0] > 0.0 && label[0] < 1920.0 && label[1] > 0.0 && label[1] < 1080.0);
}
