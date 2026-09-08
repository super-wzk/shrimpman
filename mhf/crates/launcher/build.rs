#[cfg(feature = "unicode")]
#[path = "build/resource_layout.rs"]
mod resource_layout;
#[cfg(feature = "translation")]
#[path = "build/translation_dictionary.rs"]
mod translation_dictionary;

fn main() {
    #[cfg(feature = "unicode")]
    if let Err(error) = generate() {
        panic!("failed to generate MHF text resources: {error}");
    }
}

#[cfg(feature = "unicode")]
fn generate() -> Result<(), String> {
    use std::{env, path::PathBuf};

    let manifest_directory = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| "CARGO_MANIFEST_DIR is not set".to_owned())?,
    );
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").ok_or_else(|| "OUT_DIR is not set".to_owned())?);
    let _catalog = resource_layout::generate(&manifest_directory, &output_directory)?;
    #[cfg(feature = "translation")]
    translation_dictionary::generate(
        &manifest_directory.join("translations"),
        &output_directory,
        &_catalog,
    )?;
    Ok(())
}
