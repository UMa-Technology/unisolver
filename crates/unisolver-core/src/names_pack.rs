//! Multilingual names pack (`UNAM` file): object key → {language → name}.
//!
//! A separate file rather than part of the DSO catalog because it covers deep-sky
//! objects, named stars and solar-system bodies alike, and a new release of name
//! data should not require regenerating the catalog.
//!
//! The language set comes from the data (`languages()`), not from the engine.
//! Lookup falls back: requested language → English → the caller's own fallback
//! (usually the designation).
use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const MAGIC: &[u8; 4] = b"UNAM";
const VERSION: u8 = 1;

/// On disk: the language list plus, per key, one name per language (None = missing).
/// Column layout avoids repeating language codes; 965 objects × 13 languages ≈ 250 KB.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NamesPack {
    /// Language codes as in the source data (`en` / `zh_cn` / `zh_tw` / `ja` / …)
    pub languages: Vec<String>,
    /// Key → names per language (length must equal `languages`)
    pub entries: HashMap<String, Vec<Option<String>>>,
}

impl NamesPack {
    pub fn write(&self, path: &str) -> Result<()> {
        // Serialize through a key-sorted view so the same content always gives
        // the same bytes (HashMap order is random). postcard encodes both map
        // types identically, so `open` still reads into the HashMap.
        #[derive(Serialize)]
        struct Sorted<'a> {
            languages: &'a [String],
            entries: std::collections::BTreeMap<&'a str, &'a Vec<Option<String>>>,
        }
        let sorted = Sorted {
            languages: &self.languages,
            entries: self.entries.iter().map(|(k, v)| (k.as_str(), v)).collect(),
        };
        let mut buf = Vec::with_capacity(self.entries.len() * 96 + 5);
        buf.extend_from_slice(MAGIC);
        buf.push(VERSION);
        buf.extend(
            postcard::to_allocvec(&sorted).map_err(|e| CoreError::InvalidInput(e.to_string()))?,
        );
        std::fs::write(path, buf)?;
        Ok(())
    }

    pub fn open(path: &str) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < 5 || &bytes[0..4] != MAGIC {
            return Err(CoreError::InvalidInput(format!("{path}: not a UNAM file")));
        }
        if bytes[4] != VERSION {
            return Err(CoreError::InvalidInput(format!(
                "{path}: unsupported UNAM version {}",
                bytes[4]
            )));
        }
        let pack: NamesPack = postcard::from_bytes(&bytes[5..])
            .map_err(|e| CoreError::InvalidInput(format!("{path}: {e}")))?;
        for (k, v) in &pack.entries {
            if v.len() != pack.languages.len() {
                return Err(CoreError::InvalidInput(format!(
                    "{path}: {k} has {} names for {} languages",
                    v.len(),
                    pack.languages.len()
                )));
            }
        }
        Ok(pack)
    }

    /// Maps a caller's language code to a column of the pack.
    ///
    /// Lenient: `zh-CN` / `zh_Hans` / `zh-Hans-CN` / `ZH_cn` all match `zh_cn`; a bare
    /// `zh` picks the first variant in pack order (Simplified before Traditional).
    /// Returns None when nothing matches; the caller falls back to English.
    pub fn resolve_language(&self, requested: &str) -> Option<usize> {
        let want = requested.trim().to_ascii_lowercase().replace('-', "_");
        if want.is_empty() {
            return None;
        }
        if let Some(i) = self
            .languages
            .iter()
            .position(|l| l.to_ascii_lowercase() == want)
        {
            return Some(i);
        }
        // Script subtags such as zh_hans/zh_hant map to a region code
        let alias = match want.as_str() {
            "zh_hans" | "zh_hans_cn" | "zh_sg" | "zh_cn" => Some("zh_cn"),
            "zh_hant" | "zh_hant_tw" | "zh_hk" | "zh_mo" | "zh_tw" => Some("zh_tw"),
            _ => None,
        };
        if let Some(a) = alias {
            if let Some(i) = self.languages.iter().position(|l| l == a) {
                return Some(i);
            }
        }
        // Primary language only ("zh" / "pt"): take the first variant in the pack
        let prefix = format!("{}_", want.split('_').next().unwrap_or(&want));
        self.languages
            .iter()
            .position(|l| l.to_ascii_lowercase().starts_with(&prefix))
    }

    /// Name in the requested language, else English, else None (callers usually
    /// fall back to the designation).
    pub fn name(&self, key: &str, lang_idx: Option<usize>) -> Option<&str> {
        let row = self.entries.get(key)?;
        let en = self.languages.iter().position(|l| l == "en");
        lang_idx
            .and_then(|i| row.get(i).and_then(|o| o.as_deref()))
            .or_else(|| en.and_then(|i| row.get(i).and_then(|o| o.as_deref())))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> NamesPack {
        let mut entries = HashMap::new();
        entries.insert(
            "M31".to_string(),
            vec![
                Some("Andromeda Galaxy".into()),
                Some("仙女座星系".into()),
                None,
            ],
        );
        entries.insert(
            "NGC0891".to_string(),
            vec![Some("Silver Sliver".into()), None, None],
        );
        NamesPack {
            languages: vec!["en".into(), "zh_cn".into(), "ja".into()],
            entries,
        }
    }

    #[test]
    fn language_codes_are_matched_leniently() {
        let p = pack();
        for code in ["zh_cn", "zh-CN", "ZH_cn", "zh-Hans", "zh_Hans_CN", "zh"] {
            assert_eq!(p.resolve_language(code), Some(1), "failed for {code}");
        }
        assert_eq!(p.resolve_language("en"), Some(0));
        assert_eq!(p.resolve_language("ja"), Some(2));
        // A language the pack lacks is not guessed
        assert_eq!(p.resolve_language("de"), None);
        assert_eq!(p.resolve_language(""), None);
    }

    #[test]
    fn name_falls_back_to_english_then_nothing() {
        let p = pack();
        let ja = p.resolve_language("ja");
        assert_eq!(
            p.name("M31", p.resolve_language("zh_cn")),
            Some("仙女座星系")
        );
        // Japanese missing → English
        assert_eq!(p.name("M31", ja), Some("Andromeda Galaxy"));
        assert_eq!(
            p.name("NGC0891", p.resolve_language("zh_cn")),
            Some("Silver Sliver")
        );
        // Unknown object → None (the caller falls back to the designation)
        assert_eq!(p.name("IC4628", ja), None);
    }

    #[test]
    fn roundtrip_and_reject_bad_files() {
        let p = std::env::temp_dir().join("unam_test.bin");
        pack().write(p.to_str().unwrap()).unwrap();
        let back = NamesPack::open(p.to_str().unwrap()).unwrap();
        assert_eq!(back.languages, vec!["en", "zh_cn", "ja"]);
        assert_eq!(back.len(), 2);
        std::fs::write(&p, b"XXXX\x01junk").unwrap();
        assert!(NamesPack::open(p.to_str().unwrap()).is_err());
        // A row whose length differs from the language count is rejected on open
        let mut bad = pack();
        bad.entries.insert("X".into(), vec![None]);
        bad.write(p.to_str().unwrap()).unwrap();
        let e = NamesPack::open(p.to_str().unwrap()).unwrap_err();
        assert!(e.to_string().contains("names for"), "{e}");
    }

    /// Same content must give the same bytes: the pack is a checked-in asset,
    /// and regenerating it must be verifiable with a byte compare.
    #[test]
    fn writes_are_byte_reproducible() {
        let build = || {
            let entries = (0..200)
                .map(|i| (format!("NGC{i:04}"), vec![Some(format!("n{i}"))]))
                .collect();
            NamesPack {
                languages: vec!["en".into()],
                entries,
            }
        };
        let dir = std::env::temp_dir();
        let (a, b) = (dir.join("unam_det_a.bin"), dir.join("unam_det_b.bin"));
        build().write(a.to_str().unwrap()).unwrap();
        build().write(b.to_str().unwrap()).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    }
}
