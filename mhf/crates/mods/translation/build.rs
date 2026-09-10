#[cfg(feature = "provider")]
#[path = "build/dictionary.rs"]
mod dictionary;
#[cfg(feature = "provider")]
#[allow(dead_code)]
#[path = "../unicode/build/resource_layout.rs"]
mod resource_layout;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    #[cfg(feature = "provider")]
    {
        let directory = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let unicode = directory.join("../unicode");
        println!(
            "cargo::rerun-if-changed={}",
            unicode.join("build/resource_layout.rs").display()
        );
        let catalog = resource_layout::read_catalog(&unicode)
            .expect("failed to read the shared Unicode catalog");
        dictionary::generate(&directory.join("locales"), &output, &catalog)
            .expect("failed to generate translation dictionaries");
    }
}
