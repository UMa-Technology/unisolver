//! Prints every annotated solar-system body at a given time (for comparison with astropy):
//!     cargo run --release -p unisolver-core --example probe_solar -- <unix_ms>
fn main() {
    let ms: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    for b in unisolver_core::ephemeris::solar_system_positions(ms) {
        println!("{:10} {:9.4} {:9.4}", b.name_en, b.ra_deg, b.dec_deg);
    }
}
