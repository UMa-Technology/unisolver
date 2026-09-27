fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    if let Ok(b) = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml")).unwrap())
        .generate()
    {
        b.write_to_file(format!("{crate_dir}/include/unisolver.h"));
    }
}
