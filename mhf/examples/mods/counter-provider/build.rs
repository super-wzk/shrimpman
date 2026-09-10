use std::{env, path::PathBuf};

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    let include_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("include");
    example_counter_sdk::generate_headers(&include_dir)
}
