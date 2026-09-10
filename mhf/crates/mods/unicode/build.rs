#[cfg(feature = "provider")]
#[allow(dead_code)]
#[path = "build/resource_layout.rs"]
mod resource_layout;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    #[cfg(feature = "provider")]
    {
        let directory = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        resource_layout::generate(&directory, &output)
            .expect("failed to generate Unicode resource layout");
    }
}
