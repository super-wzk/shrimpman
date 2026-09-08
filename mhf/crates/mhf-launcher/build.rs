#[cfg(feature = "translation")]
#[path = "build/translation_dictionary.rs"]
mod translation_dictionary;

fn main() {
    #[cfg(feature = "translation")]
    if let Err(error) = translation_dictionary::generate() {
        panic!("failed to generate MHF text resources: {error}");
    }
}
