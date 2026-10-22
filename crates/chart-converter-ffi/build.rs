fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-env-changed=CHART_CONVERTER_HEADER_DIR");
    let destination = std::env::var_os("CHART_CONVERTER_HEADER_DIR")
        .unwrap_or_else(|| std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::create_dir_all(&destination).expect("create C API header directory");
    let mut config = cbindgen::Config {
        language: cbindgen::Language::C,
        include_guard: Some("CHART_CONVERTER_H".into()),
        usize_is_size_t: true,
        cpp_compat: true,
        ..Default::default()
    };
    config.enumeration.prefix_with_name = true;
    config.export.include = vec!["CcFormat".into(), "CcMode".into()];
    cbindgen::Builder::new()
        .with_crate(std::env::var("CARGO_MANIFEST_DIR").expect("Cargo crate directory"))
        .with_config(config)
        .generate()
        .expect("generate C API header")
        .write_to_file(std::path::PathBuf::from(destination).join("chart_converter.h"));
}
