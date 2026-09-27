//! Builds the multilingual names pack (`UNAM`):
//!     cargo run --release -p namesgen -- --input data/objects-names.json \
//!         --output packages/unisolver_flutter/lib/optional/unisolver_names.bin
//!
//! The input is `objects-names.json` exported by the objects_catalogs project (each row
//! `[id, "designation1,designation2", "en:Name", "zh_cn:Name", …]`), derived from
//! Stellarium's names.dat / common_star_names.fab and the stellarium-sky .po translations.
//!
//! Keys are normalized to unisolver's designations: `NGC 224` → `NGC0224`, `M 31` → `M31`,
//! `IC 405` → `IC0405`, `HIP 677` → `HIP677`. **Every** designation of a record becomes a key
//! (Messier objects are `M31` in our DSO catalog but may be `NGC0224` elsewhere; both find the
//! same names).
//!
//! Solar-system bodies are not in the input (it has only deep-sky objects and stars); a small
//! built-in table adds them.
use clap::Parser;
use std::collections::{BTreeSet, HashMap};
use unisolver_core::names_pack::NamesPack;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    input: std::path::PathBuf,
    #[arg(long)]
    output: std::path::PathBuf,
    /// Print statistics without writing a file
    #[arg(long)]
    dry_run: bool,
}

/// Solar-system names (absent from the input). Keys match `ephemeris`'s `name_en`.
/// Order: en, zh_cn, zh_tw, ja, ko, fr, de, es, it, ru, pl, hu, ro
const SOLAR: &[(&str, [&str; 13])] = &[
    (
        "Sun",
        [
            "Sun",
            "太阳",
            "太陽",
            "太陽",
            "태양",
            "Soleil",
            "Sonne",
            "Sol",
            "Sole",
            "Солнце",
            "Słońce",
            "Nap",
            "Soare",
        ],
    ),
    (
        "Moon",
        [
            "Moon",
            "月亮",
            "月球",
            "月",
            "달",
            "Lune",
            "Mond",
            "Luna",
            "Luna",
            "Луна",
            "Księżyc",
            "Hold",
            "Luna",
        ],
    ),
    (
        "mercury",
        [
            "Mercury",
            "水星",
            "水星",
            "水星",
            "수성",
            "Mercure",
            "Merkur",
            "Mercurio",
            "Mercurio",
            "Меркурий",
            "Merkury",
            "Merkúr",
            "Mercur",
        ],
    ),
    (
        "venus",
        [
            "Venus",
            "金星",
            "金星",
            "金星",
            "금성",
            "Vénus",
            "Venus",
            "Venus",
            "Venere",
            "Венера",
            "Wenus",
            "Vénusz",
            "Venus",
        ],
    ),
    (
        "mars",
        [
            "Mars", "火星", "火星", "火星", "화성", "Mars", "Mars", "Marte", "Marte", "Марс",
            "Mars", "Mars", "Marte",
        ],
    ),
    (
        "jupiter",
        [
            "Jupiter",
            "木星",
            "木星",
            "木星",
            "목성",
            "Jupiter",
            "Jupiter",
            "Júpiter",
            "Giove",
            "Юпитер",
            "Jowisz",
            "Jupiter",
            "Jupiter",
        ],
    ),
    (
        "saturn",
        [
            "Saturn",
            "土星",
            "土星",
            "土星",
            "토성",
            "Saturne",
            "Saturn",
            "Saturno",
            "Saturno",
            "Сатурн",
            "Saturn",
            "Szaturnusz",
            "Saturn",
        ],
    ),
    (
        "uranus",
        [
            "Uranus",
            "天王星",
            "天王星",
            "天王星",
            "천왕성",
            "Uranus",
            "Uranus",
            "Urano",
            "Urano",
            "Уран",
            "Uran",
            "Uránusz",
            "Uranus",
        ],
    ),
    (
        "neptune",
        [
            "Neptune",
            "海王星",
            "海王星",
            "海王星",
            "해왕성",
            "Neptune",
            "Neptun",
            "Neptuno",
            "Nettuno",
            "Нептун",
            "Neptun",
            "Neptunusz",
            "Neptun",
        ],
    ),
    (
        "pluto",
        [
            "Pluto",
            "冥王星",
            "冥王星",
            "冥王星",
            "명왕성",
            "Pluton",
            "Pluto",
            "Plutón",
            "Plutone",
            "Плутон",
            "Pluton",
            "Plútó",
            "Pluto",
        ],
    ),
];
const SOLAR_LANGS: [&str; 13] = [
    "en", "zh_cn", "zh_tw", "ja", "ko", "fr", "de", "es", "it", "ru", "pl", "hu", "ro",
];

/// `NGC 224` → `NGC0224`; `M 31` → `M31`; `HIP 677` → `HIP677`.
/// Other catalogs (PGC/UGC/SH2/…) are not in our tables and are skipped.
fn normalize(designation: &str) -> Option<String> {
    let d = designation.trim();
    let (cat, num) = d.split_once(' ')?;
    let num = num.trim();
    if !num.chars().all(|c| c.is_ascii_digit()) {
        return None; // suffixed components (NGC 1234A) do not match our tables
    }
    let n: u32 = num.parse().ok()?;
    match cat.to_ascii_uppercase().as_str() {
        "NGC" => Some(format!("NGC{n:04}")),
        "IC" => Some(format!("IC{n:04}")),
        "M" => Some(format!("M{n}")),
        "HIP" => Some(format!("HIP{n}")),
        _ => None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let raw: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cli.input)?)?;
    let rows = raw.as_array().ok_or("the input must be an array")?;

    // First pass: collect the language set (the data decides, the engine does not hard-code it)
    let mut langs: BTreeSet<String> = BTreeSet::new();
    for r in rows {
        for f in r.as_array().into_iter().flatten().skip(2) {
            if let Some((l, _)) = f.as_str().and_then(|s| s.split_once(':')) {
                langs.insert(l.to_string());
            }
        }
    }
    for l in SOLAR_LANGS {
        langs.insert(l.to_string());
    }
    // English goes first (the fallback column)
    let mut languages: Vec<String> = langs.into_iter().collect();
    if let Some(i) = languages.iter().position(|l| l == "en") {
        languages.swap(0, i);
    }
    let col: HashMap<&str, usize> = languages
        .iter()
        .enumerate()
        .map(|(i, l)| (l.as_str(), i))
        .collect();

    let mut entries: HashMap<String, Vec<Option<String>>> = HashMap::new();
    let (mut rows_with_names, mut keys_skipped) = (0usize, 0usize);
    for r in rows {
        let a = match r.as_array() {
            Some(a) if a.len() > 2 => a,
            _ => continue,
        };
        rows_with_names += 1;
        let mut names = vec![None; languages.len()];
        for f in a.iter().skip(2) {
            if let Some((l, n)) = f.as_str().and_then(|s| s.split_once(':')) {
                if let Some(&i) = col.get(l) {
                    names[i] = Some(n.trim().to_string());
                }
            }
        }
        let designations = a[1].as_str().unwrap_or("");
        let mut hit = false;
        for d in designations.split(',') {
            if let Some(key) = normalize(d) {
                hit = true;
                // A key may be hit by several rows (the data sometimes splits one object across rows with
                // different names): first come, first served, since earlier rows carry the main name
                entries.entry(key).or_insert_with(|| names.clone());
            }
        }
        if !hit {
            keys_skipped += 1;
        }
    }

    // Solar-system table
    for (key, names) in SOLAR {
        let mut row = vec![None; languages.len()];
        for (l, n) in SOLAR_LANGS.iter().zip(names.iter()) {
            if let Some(&i) = col.get(l) {
                row[i] = Some((*n).to_string());
            }
        }
        entries.insert((*key).to_string(), row);
    }

    let pack = NamesPack {
        languages: languages.clone(),
        entries,
    };
    let stars = pack.entries.keys().filter(|k| k.starts_with("HIP")).count();
    println!(
        "{} languages: {}\n{} keys ({} stars, {} others); {} named rows in the source, {} with designations outside our catalogs",
        languages.len(),
        languages.join(" "),
        pack.len(),
        stars,
        pack.len() - stars,
        rows_with_names,
        keys_skipped
    );
    if cli.dry_run {
        return Ok(());
    }
    pack.write(cli.output.to_str().unwrap())?;
    println!(
        "→ {} ({:.0} KB)",
        cli.output.display(),
        std::fs::metadata(&cli.output)?.len() as f64 / 1024.0
    );
    Ok(())
}
