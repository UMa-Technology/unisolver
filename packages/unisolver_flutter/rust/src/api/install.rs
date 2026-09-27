use anyhow::{Context, Result};

/// Decompresses a zstd database to `out_path`; skips when the target exists and is non-empty.
/// Returns the decompressed size in bytes.
pub fn install_compressed_db(zst_path: String, out_path: String) -> Result<u64> {
    if let Ok(m) = std::fs::metadata(&out_path) {
        if m.len() > 0 {
            return Ok(m.len());
        }
    }
    // Streaming end to end: neither the .zst nor the output is held in memory. A buffered
    // implementation peaked at 1.59 GB for N2 (119 MB zst → 322 MB database), which mobile
    // would kill; streaming needs only the zstd window plus IO buffers (a few MB).
    let src = std::fs::File::open(&zst_path).with_context(|| format!("read {zst_path}"))?;
    let mut decoder =
        ruzstd::decoding::StreamingDecoder::new(std::io::BufReader::with_capacity(1 << 20, src))
            .context("zstd stream")?;
    if let Some(dir) = std::path::Path::new(&out_path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = format!("{out_path}.tmp");
    let n = {
        let file = std::fs::File::create(&tmp).with_context(|| format!("create {tmp}"))?;
        let mut sink = std::io::BufWriter::with_capacity(1 << 20, file);
        let n = std::io::copy(&mut decoder, &mut sink).context("zstd decode")?;
        std::io::Write::flush(&mut sink)?;
        // Sync before the rename, so a power loss never leaves a renamed but half-written database
        sink.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        n
    };
    std::fs::rename(&tmp, &out_path)?; // atomic: a partial file is never taken as installed
    Ok(n)
}

/// Streaming sha256 of a file (lowercase hex), for download verification: a tier of several
/// hundred MB should not be read into memory on the Dart side to hash it.
pub fn sha256_file(path: String) -> Result<String> {
    use sha2::Digest;
    let f = std::fs::File::open(&path).with_context(|| format!("read {path}"))?;
    let mut r = std::io::BufReader::with_capacity(1 << 20, f);
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = std::io::Read::read(&mut r, &mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Maintainer data: `UNISOLVER_TIER_DIR` names a directory of tier databases (`X.db`), their
    /// archives (`X.db.zst`; the bundled tier's archive is the plugin asset) and the published
    /// `manifest.json`. Unset on a clean clone, so the two tests below skip.
    fn tier_dir() -> Option<std::path::PathBuf> {
        let d = std::path::PathBuf::from(std::env::var_os("UNISOLVER_TIER_DIR")?);
        Some(if d.is_absolute() {
            d
        } else {
            repo_root().join(d)
        })
    }

    fn repo_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    /// An archive by file name: in the tier directory, else among the plugin's bundled assets.
    fn archive(dir: &std::path::Path, file: &str) -> Option<std::path::PathBuf> {
        [
            dir.join(file),
            repo_root()
                .join("packages/unisolver_flutter/assets")
                .join(file),
        ]
        .into_iter()
        .find(|p| p.exists())
    }

    /// Larger tiers are skipped to keep the gate fast (the deepest takes tens of seconds).
    const MAX_TEST_BYTES: u64 = 400 << 20;

    /// Decodes the real tier archives: dbgen writes **zstd level 19** while devices decode with
    /// pure-Rust ruzstd, and the test above only covers level 3 on synthetic data. On a
    /// maintainer machine it locks "what we ship decodes on devices".
    #[test]
    fn real_tier_archives_decode_byte_identically() {
        let Some(dir) = tier_dir() else {
            eprintln!("skipped: UNISOLVER_TIER_DIR not set");
            return;
        };
        let mut raws: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "db"))
            .collect();
        raws.sort();
        let mut ran = 0;
        for raw in raws {
            let file = format!("{}.zst", raw.file_name().unwrap().to_string_lossy());
            let Some(zst) = archive(&dir, &file) else {
                continue;
            };
            if std::fs::metadata(&raw).unwrap().len() > MAX_TEST_BYTES {
                continue;
            }
            let out = std::env::temp_dir().join(format!(
                "frb_real_{}",
                zst.file_name().unwrap().to_string_lossy()
            ));
            let _ = std::fs::remove_file(&out);
            let n =
                install_compressed_db(zst.to_str().unwrap().into(), out.to_str().unwrap().into())
                    .unwrap_or_else(|e| panic!("ruzstd cannot decode {}: {e:#}", zst.display()));
            let expect = std::fs::metadata(&raw).unwrap().len();
            assert_eq!(n, expect, "{}: decoded size differs", zst.display());
            // Compare block by block rather than reading two files of hundreds of MB
            let mut a = std::io::BufReader::new(std::fs::File::open(&out).unwrap());
            let mut b = std::io::BufReader::new(std::fs::File::open(&raw).unwrap());
            let (mut ba, mut bb) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
            loop {
                let ra = std::io::Read::read(&mut a, &mut ba).unwrap();
                let rb = std::io::Read::read(&mut b, &mut bb).unwrap();
                assert_eq!(ra, rb);
                if ra == 0 {
                    break;
                }
                assert_eq!(
                    ba[..ra],
                    bb[..rb],
                    "{}: decoded bytes differ from the original",
                    zst.display()
                );
            }
            let _ = std::fs::remove_file(&out);
            ran += 1;
        }
        assert!(ran > 0, "no tier archive under {}", dir.display());
        eprintln!("real_tier_archives_decode_byte_identically: checked {ran} tier(s)");
    }

    #[test]
    fn sha256_matches_the_known_digest_of_an_empty_and_a_small_file() {
        let dir = std::env::temp_dir().join("frb_sha");
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty.bin");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(
            sha256_file(empty.to_str().unwrap().into()).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let abc = dir.join("abc.bin");
        std::fs::write(&abc, b"abc").unwrap();
        assert_eq!(
            sha256_file(abc.to_str().unwrap().into()).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // A missing file is an error, not an empty digest
        assert!(sha256_file(dir.join("nope.bin").to_str().unwrap().into()).is_err());
    }

    /// Real tiers: the manifest's sha256 must equal this function's result, or every client's
    /// post-download check would fail (the upload script uses the same digest).
    #[test]
    fn sha256_agrees_with_the_manifest_for_real_tiers() {
        let Some(dir) = tier_dir() else {
            eprintln!("skipped: UNISOLVER_TIER_DIR not set");
            return;
        };
        let m: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                .unwrap();
        let mut ran = 0;
        for t in m["tiers"].as_array().unwrap() {
            let (Some(file), Some(want)) = (t["file"].as_str(), t["sha256"].as_str()) else {
                continue;
            };
            let Some(zst) = archive(&dir, file) else {
                continue;
            };
            if std::fs::metadata(&zst).unwrap().len() > MAX_TEST_BYTES {
                continue;
            }
            assert_eq!(
                sha256_file(zst.to_str().unwrap().into()).unwrap(),
                want,
                "{file}"
            );
            ran += 1;
        }
        assert!(ran > 0, "no manifest tier found under {}", dir.display());
        eprintln!("sha256_agrees_with_the_manifest_for_real_tiers: checked {ran} tier(s)");
    }

    #[test]
    fn install_decompresses_and_is_idempotent() {
        let dir = std::env::temp_dir().join("frb_install");
        std::fs::create_dir_all(&dir).unwrap();
        let raw: Vec<u8> = (0..50_000u32).flat_map(|v| v.to_le_bytes()).collect();
        let zst = dir.join("x.zst");
        std::fs::write(&zst, zstd::encode_all(&raw[..], 3).unwrap()).unwrap();
        let out = dir.join("x.bin");
        let _ = std::fs::remove_file(&out);
        let n = install_compressed_db(zst.to_str().unwrap().into(), out.to_str().unwrap().into())
            .unwrap();
        assert_eq!(n as usize, raw.len());
        assert_eq!(std::fs::read(&out).unwrap(), raw);
        // Idempotent: calling again returns the existing size without rewriting
        assert_eq!(
            install_compressed_db(zst.to_str().unwrap().into(), out.to_str().unwrap().into())
                .unwrap() as usize,
            raw.len()
        );
    }
}
