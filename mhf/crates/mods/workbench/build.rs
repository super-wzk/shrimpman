#[allow(dead_code)]
#[path = "../../shared/resource/build/resource_layout.rs"]
mod resource_layout;

fn main() {
    let directory = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let resource = directory.join("../../shared/resource");
    println!("cargo::rerun-if-changed=build.rs");
    println!(
        "cargo::rerun-if-changed={}",
        resource.join("build/resource_layout.rs").display()
    );
    resource_layout::generate_inspection(&resource, &output)
        .expect("failed to generate resource inspection layouts");
}
