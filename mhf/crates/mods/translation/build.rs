#[cfg(feature = "provider")]
#[path = "build/dictionary.rs"]
mod dictionary;
#[cfg(feature = "provider")]
#[allow(dead_code)]
#[path = "../../shared/resource/build/resource_layout.rs"]
mod resource_layout;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    #[cfg(feature = "provider")]
    {
        let directory = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let resource = directory.join("../../shared/resource");
        println!(
            "cargo::rerun-if-changed={}",
            resource.join("build/resource_layout.rs").display()
        );
        let catalog = resource_layout::read_catalog(&resource)
            .expect("failed to read the shared resource catalog");
        dictionary::generate(&directory.join("locales"), &output, &catalog)
            .expect("failed to generate translation dictionaries");
    }
}
