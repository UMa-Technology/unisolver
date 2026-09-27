//! Attributions for the data the engine ships or reads. Several sources require an
//! attribution in the app that uses them (Gaia DR3 in particular), so apps read the
//! text from here instead of hard-coding it.

use serde::Serialize;

/// One data source and what its license asks of an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DataAttribution {
    /// Stable identifier (`gaia`, `hipparcos`, `iau_wgsn`, `openngc`, `stellarium`)
    pub id: &'static str,
    pub name: &'static str,
    /// What in the engine derives from it
    pub applies_to: &'static str,
    /// SPDX-style license identifier
    pub license: &'static str,
    /// Attribution text to show in the app
    pub text: &'static str,
    pub url: &'static str,
}

/// Every data source in the engine's databases, catalogs and names pack.
pub fn data_attributions() -> &'static [DataAttribution] {
    &ATTRIBUTIONS
}

static ATTRIBUTIONS: [DataAttribution; 5] = [
    DataAttribution {
        id: "gaia",
        name: "ESA Gaia DR3",
        applies_to: "star databases",
        license: "CC-BY-SA-3.0-IGO",
        text: "This work has made use of data from the European Space Agency (ESA) mission Gaia \
               (https://www.cosmos.esa.int/gaia), processed by the Gaia Data Processing and \
               Analysis Consortium (DPAC).",
        url: "https://www.cosmos.esa.int/gaia",
    },
    DataAttribution {
        id: "hipparcos",
        name: "Hipparcos, the new reduction (van Leeuwen 2007)",
        applies_to: "star databases (bright stars)",
        license: "CDS terms of use",
        text: "Bright stars from the Hipparcos new reduction (van Leeuwen 2007), \
               via the CDS VizieR catalogue I/311.",
        url: "https://cdsarc.cds.unistra.fr/viz-bin/cat/I/311",
    },
    DataAttribution {
        id: "iau_wgsn",
        name: "IAU Catalog of Star Names",
        applies_to: "named stars",
        license: "IAU",
        text: "Star names from the IAU Working Group on Star Names (WGSN).",
        url: "https://www.iau.org/public/themes/naming_stars/",
    },
    DataAttribution {
        id: "openngc",
        name: "OpenNGC",
        applies_to: "deep-sky catalog",
        license: "CC-BY-SA-4.0",
        text: "Deep-sky object data from OpenNGC by Mattia Verga (CC BY-SA 4.0).",
        url: "https://github.com/mattiaverga/OpenNGC",
    },
    DataAttribution {
        id: "stellarium",
        name: "Stellarium",
        applies_to: "multilingual names pack",
        license: "GPL-2.0-or-later",
        text: "Object names and translations from Stellarium (GPL-2.0-or-later).",
        url: "https://stellarium.org",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_is_listed_once_with_its_terms() {
        let all = data_attributions();
        let ids: Vec<&str> = all.iter().map(|a| a.id).collect();
        assert_eq!(
            ids,
            ["gaia", "hipparcos", "iau_wgsn", "openngc", "stellarium"]
        );
        for a in all {
            assert!(!a.name.is_empty() && !a.text.is_empty() && !a.license.is_empty());
            assert!(a.url.starts_with("https://"), "{}", a.url);
        }
        let gaia = all.iter().find(|a| a.id == "gaia").unwrap();
        assert!(gaia
            .text
            .contains("European Space Agency (ESA) mission Gaia"));
        assert_eq!(gaia.license, "CC-BY-SA-3.0-IGO");
        let st = all.iter().find(|a| a.id == "stellarium").unwrap();
        assert_eq!(st.license, "GPL-2.0-or-later");
    }
}
