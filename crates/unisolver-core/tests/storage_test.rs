//! Database files: tetra3's format 2, memory-mapped (patch 0003 in third_party/tetra3-patches:
//! the packed pattern section stays on the file and pages in as solves touch it), files from
//! older engines refused with a clear message, corrupt files failing or solving safely.
use tetra3::solver::SolverDatabase;
use unisolver_core::{Frame, PixelData, SolveOptions, SolveStatus, Solver};
use unisolver_synth as synth;

fn tmp(name: &str) -> String {
    std::env::temp_dir()
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
}

fn solve_with(db_path: &str) -> unisolver_core::SolveOutcome {
    let solver = Solver::from_file(db_path).unwrap();
    let q = synth::look_at(45.0, 25.0, 0.0);
    let img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        20.0,
        1024,
        768,
        &synth::RenderParams::default(),
        9,
    );
    let frame = Frame {
        width: 1024,
        height: 768,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(img),
    };
    solver.solve(&frame, &SolveOptions::new(20.0)).unwrap()
}

#[test]
fn a_mapped_file_holds_the_database_it_was_saved_from() {
    let db = synth::test_db();
    let path = tmp("unisolver_storage_format2.db");
    db.save_to_file(&path).unwrap();
    assert_eq!(&std::fs::read(&path).unwrap()[0..6], b"T3DB\x02\x00");

    let mapped = SolverDatabase::open_mapped(&path).unwrap();
    assert_eq!(mapped.star_catalog_ids, db.star_catalog_ids);
    assert_eq!(mapped.star_vectors.len(), db.star_vectors.len());
    assert_eq!(mapped.props.num_patterns, db.props.num_patterns);
    assert_eq!(mapped.pattern_catalog, db.pattern_catalog);
    assert!(
        format!("{:?}", mapped.pattern_catalog).contains("mapped"),
        "the packed section must stay on the file: {:?}",
        mapped.pattern_catalog
    );

    let out = solve_with(&path);
    assert!(matches!(out.status, SolveStatus::Ok), "{:?}", out.status);
    let g = out.solution.unwrap();
    assert!((g.ra_deg - 45.0).abs() < 0.1 && (g.dec_deg - 25.0).abs() < 0.1);
}

#[test]
fn old_format_files_are_refused_with_a_clear_message() {
    let path = tmp("unisolver_storage_old.db");
    let mut bytes = b"UNISOLV2".to_vec();
    bytes.extend([0u8; 64]);
    std::fs::write(&path, bytes).unwrap();
    let Err(e) = Solver::from_file(&path) else {
        panic!("an old-format file must be refused");
    };
    let e = e.to_string();
    assert!(
        e.contains("old-format") && e.contains("download it again"),
        "{e}"
    );
}

#[test]
fn corrupt_files_fail_or_solve_safely() {
    let db = synth::test_db();
    let path = tmp("unisolver_storage_corrupt.db");
    db.save_to_file(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();

    // Truncated, or garbage: the decode fails
    let cut = tmp("unisolver_storage_cut.db");
    std::fs::write(&cut, &bytes[..bytes.len() - 64]).unwrap();
    assert!(SolverDatabase::open_mapped(&cut).is_err());
    let junk = tmp("unisolver_storage_junk.db");
    std::fs::write(&junk, [0xFFu8; 256]).unwrap();
    assert!(SolverDatabase::open_mapped(&junk).is_err());

    // A star index past the table: the full check refuses it; a mapped load skips that sweep
    // (it would read the whole table), so it loads, and the probe-time guard keeps solves safe
    let (_, first) = db.pattern_catalog.iter().next().unwrap();
    let mut packed = Vec::new();
    for i in first.star_indices {
        packed.extend(i.to_le_bytes());
    }
    packed.extend(first.largest_edge.to_le_bytes());
    packed.extend(first.key_hash.to_le_bytes());
    let at = bytes
        .windows(packed.len())
        .position(|w| w == packed.as_slice())
        .expect("the first packed entry is in the file");
    let mut bad = bytes.clone();
    bad[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let bad_path = tmp("unisolver_storage_bad_index.db");
    std::fs::write(&bad_path, &bad).unwrap();
    assert!(SolverDatabase::load_from_file(&bad_path).is_err());
    assert!(SolverDatabase::open_mapped(&bad_path).is_ok());
    let _ = solve_with(&bad_path);
}
