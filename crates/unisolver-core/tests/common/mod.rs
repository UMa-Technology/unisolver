//! Helpers shared by the golden regressions.
#![allow(dead_code)]
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The bundled database, decompressed once per test process into target/golden/.
pub fn bundled_w_db() -> &'static Path {
    static DB: OnceLock<PathBuf> = OnceLock::new();
    DB.get_or_init(|| {
        let root = repo_root();
        let zst = root.join("packages/unisolver_flutter/assets/unisolver_10_80.db.zst");
        let dir = root.join("target/golden");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("unisolver_10_80.db");
        let fresh = std::fs::metadata(&out)
            .and_then(|m| Ok((m, std::fs::metadata(&zst)?)))
            .map(|(db, z)| db.modified().ok() >= z.modified().ok() && db.len() > 0)
            .unwrap_or(false);
        if !fresh {
            // Write, then rename: parallel test binaries must never see a half-written file
            let tmp = dir.join(format!("unisolver_10_80.db.{}", std::process::id()));
            let mut dec =
                ruzstd::decoding::StreamingDecoder::new(std::fs::File::open(&zst).unwrap())
                    .unwrap();
            let mut f = std::fs::File::create(&tmp).unwrap();
            std::io::copy(&mut dec, &mut f).unwrap();
            std::fs::rename(&tmp, &out).unwrap();
        }
        out
    })
}
