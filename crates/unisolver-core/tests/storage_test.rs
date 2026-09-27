//! UNISOLV2 mmap container: v2 write/read equivalence, v1 compatibility, corrupt files.
//! Background in third_party/tetra3/PATCHES.md #3: a deep database loaded whole with postcard
//! stays resident at 1.1 GB; v2 leaves the pattern table (nearly all of it) on disk, paged on demand.
use tetra3::solver::SolverDatabase;
use unisolver_core::{Frame, PixelData, SolveOptions, SolveStatus, SolvedGeometry, Solver};
use unisolver_synth as synth;

fn tmp(name: &str) -> String {
    std::env::temp_dir()
        .join(name)
        .to_str()
        .unwrap()
        .to_string()
}

fn render_20deg() -> (Vec<f32>, u32, u32) {
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
    (img, 1024, 768)
}

fn solve_with(db_path: &str) -> SolvedGeometry {
    let solver = Solver::from_file(db_path).unwrap();
    let (img, w, h) = render_20deg();
    let frame = Frame {
        width: w,
        height: h,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(img),
    };
    let out = solver.solve(&frame, &SolveOptions::new(20.0)).unwrap();
    assert!(matches!(out.status, SolveStatus::Ok), "{:?}", out.status);
    out.solution.unwrap()
}

#[test]
fn v2_mmap_load_solves_identically_to_owned() {
    let db = synth::test_db();

    // v2 (save_to_file_v2 writes UNISOLV2 → load takes the mmap branch)
    let v2 = tmp("unisolver_storage_v2.db");
    db.save_to_file_v2(&v2).unwrap();
    let head = std::fs::read(&v2).unwrap();
    assert_eq!(&head[0..8], b"UNISOLV2", "save_to_file_v2 must write v2");

    // Upstream format ("T3DB" header + whole postcard blob → load takes the compatible branch)
    let v1 = tmp("unisolver_storage_v1.db");
    std::fs::write(&v1, db.to_bytes().unwrap()).unwrap();
    assert_eq!(
        &std::fs::read(&v1).unwrap()[0..4],
        b"T3DB",
        "to_bytes must stay the upstream wire format"
    );

    let s2 = solve_with(&v2);
    let s1 = solve_with(&v1);
    // Same database, same frame: both load paths must give the same solution
    assert!(
        (s1.ra_deg - s2.ra_deg).abs() < 1e-9,
        "{} vs {}",
        s1.ra_deg,
        s2.ra_deg
    );
    assert!((s1.dec_deg - s2.dec_deg).abs() < 1e-9);
    assert_eq!(s1.num_matches, s2.num_matches);
}

#[test]
fn v2_roundtrip_preserves_head_fields() {
    let db = synth::test_db();
    let path = tmp("unisolver_storage_head.db");
    db.save_to_file_v2(&path).unwrap();
    let re = SolverDatabase::load_from_file(&path).unwrap();
    assert_eq!(re.star_catalog.len(), db.star_catalog.len());
    assert_eq!(re.star_vectors.len(), db.star_vectors.len());
    assert_eq!(re.star_catalog_ids, db.star_catalog_ids);
    assert_eq!(re.props.num_patterns, db.props.num_patterns);
    assert_eq!(
        re.pattern_catalog.entries.len(),
        db.pattern_catalog.entries.len()
    );
    // The pattern table is byte-identical (mmap view vs the owned table at generation)
    assert!(re
        .pattern_catalog
        .entries
        .iter()
        .zip(db.pattern_catalog.entries.iter())
        .all(|(a, b)| a == b));
}

#[test]
fn corrupt_v2_files_error_instead_of_panicking() {
    let db = synth::test_db();
    let path = tmp("unisolver_storage_corrupt.db");
    db.save_to_file_v2(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();

    // Truncated inside the pattern section: the entries length check fails
    let cut = tmp("unisolver_storage_cut.db");
    std::fs::write(&cut, &bytes[..bytes.len() - 64]).unwrap();
    assert!(SolverDatabase::load_from_file(&cut).is_err());

    // A file with only the magic
    let stub = tmp("unisolver_storage_stub.db");
    std::fs::write(&stub, b"UNISOLV2").unwrap();
    assert!(SolverDatabase::load_from_file(&stub).is_err());

    // Garbage bytes: take the v1 postcard branch and fail
    let junk = tmp("unisolver_storage_junk.db");
    std::fs::write(&junk, [0xFFu8; 256]).unwrap();
    assert!(SolverDatabase::load_from_file(&junk).is_err());
}
