fn main() {
    // Apps load the dylib through their rpath; without this the install name would be the
    // absolute path it was built at, and a copied library would not load
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libunisolver_cabi.dylib");
    }
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    if let Ok(b) = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml")).unwrap())
        .generate()
    {
        b.write_to_file(format!("{crate_dir}/include/unisolver.h"));
    }
}
