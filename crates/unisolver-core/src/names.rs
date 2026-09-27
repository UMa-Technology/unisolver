use std::sync::OnceLock;

/// IAU and common named stars (411). Only geometry and the English name live here;
/// localized names come from the names pack (`names_pack`, keyed `HIP<n>`), so a new
/// release of name data does not touch this table.
#[derive(Debug)]
pub(crate) struct NamedStar {
    pub hip: u32,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub mag: f32,
    pub name_en: &'static str,
}

pub(crate) fn named_stars() -> &'static [NamedStar] {
    static CACHE: OnceLock<Vec<NamedStar>> = OnceLock::new();
    CACHE.get_or_init(|| {
        include_str!("named_stars.csv")
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let f: Vec<&str> = l.splitn(5, ',').collect();
                assert_eq!(f.len(), 5, "named_stars.csv malformed line: {l}");
                NamedStar {
                    hip: f[0].parse().expect("hip"),
                    ra_deg: f[1].parse().expect("ra"),
                    dec_deg: f[2].parse().expect("dec"),
                    mag: f[3].parse().expect("mag"),
                    name_en: f[4],
                }
            })
            .collect()
    })
}
