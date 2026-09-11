#[allow(dead_code)]
#[path = "../unicode/build/resource_layout.rs"]
mod resource_layout;

fn main() {
    let directory = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let unicode = directory.join("../unicode");
    println!("cargo::rerun-if-changed=build.rs");
    println!(
        "cargo::rerun-if-changed={}",
        unicode.join("build/resource_layout.rs").display()
    );
    resource_layout::generate_dat_inspection(&unicode, &output)
        .expect("failed to generate DAT inspection layout");
}
