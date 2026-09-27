//! Moon position probe (used by scripts/verify/check_moon_topocentric.py):
//!     cargo run --release -p unisolver-core --example probe_moon -- <unix_ms> [lat lon alt_m]
//! Prints one JSON line: geocentric and, with an observer, topocentric apparent position.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ms: i64 = a.get(1).and_then(|s| s.parse().ok()).expect("unix_ms");
    let obs = if a.len() >= 5 {
        Some(unisolver_core::ephemeris::Observer {
            lat_deg: a[2].parse().unwrap(),
            lon_deg: a[3].parse().unwrap(),
            alt_m: a[4].parse().unwrap(),
        })
    } else {
        None
    };
    let moon = |o: Option<&unisolver_core::ephemeris::Observer>| {
        let b = unisolver_core::ephemeris::solar_system_positions_at(ms, o);
        let m = b.into_iter().find(|b| b.name_en == "Moon").unwrap();
        (m.ra_deg, m.dec_deg, m.angular_radius_deg.unwrap())
    };
    let (gra, gdec, grad) = moon(None);
    print!(
        "{{\"unix_ms\":{ms},\"geo\":{{\"ra_deg\":{gra:.6},\"dec_deg\":{gdec:.6},\"radius_deg\":{grad:.6}}}"
    );
    if let Some(o) = obs.as_ref() {
        let (tra, tdec, trad) = moon(Some(o));
        print!(",\"topo\":{{\"ra_deg\":{tra:.6},\"dec_deg\":{tdec:.6},\"radius_deg\":{trad:.6}}}");
    }
    println!("}}");
}
