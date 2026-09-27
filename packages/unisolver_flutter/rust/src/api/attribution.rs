//! Attributions for the data the engine ships or reads (Gaia DR3 requires one in the app).
use flutter_rust_bridge::frb;
use unisolver_core as core;

/// One data source and the attribution its license asks for.
pub struct DataAttributionDto {
    /// Stable identifier (`gaia`, `hipparcos`, `iau_wgsn`, `openngc`, `stellarium`)
    pub id: String,
    pub name: String,
    /// What in the engine derives from it
    pub applies_to: String,
    /// SPDX-style license identifier
    pub license: String,
    /// Text to show in the app
    pub text: String,
    pub url: String,
}

/// Every data source in the engine's databases, catalogs and names pack.
#[frb(sync)]
pub fn data_attributions() -> Vec<DataAttributionDto> {
    core::data_attributions()
        .iter()
        .map(|a| DataAttributionDto {
            id: a.id.into(),
            name: a.name.into(),
            applies_to: a.applies_to.into(),
            license: a.license.into(),
            text: a.text.into(),
            url: a.url.into(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirrors_the_core_list() {
        let v = data_attributions();
        assert_eq!(v.len(), core::data_attributions().len());
        assert!(v
            .iter()
            .any(|a| a.id == "gaia" && a.text.contains("mission Gaia")));
    }
}
