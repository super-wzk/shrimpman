#[path = "build/translation_dictionary.rs"]
mod translation_dictionary;

fn main() {
    if let Err(error) = translation_dictionary::generate() {
        panic!("failed to compile MHF translations: {error}");
    }
}
