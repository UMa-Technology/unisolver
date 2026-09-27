//! `test_db_file`: tests run in parallel, so no caller may see a half-written database.
use tetra3::SolverDatabase;

#[test]
fn parallel_callers_all_load_a_complete_file() {
    let name = format!("unisolver_synth_race_{}.db", std::process::id());
    let path = std::env::temp_dir().join(&name);
    let _ = std::fs::remove_file(&path);
    let callers: Vec<_> = (0..8)
        .map(|_| {
            let name = name.clone();
            std::thread::spawn(move || {
                let p = unisolver_synth::test_db_file(&name);
                SolverDatabase::load_from_file(&p)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
        })
        .collect();
    for c in callers {
        c.join().unwrap().unwrap();
    }
    let _ = std::fs::remove_file(&path);
}
