//! Aggregate the public domain contracts without compiling their providers.

fn main() -> std::io::Result<()> {
    use mhf_mod_api::headers as h;
    println!("cargo::rerun-if-changed=build.rs");
    let include =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR"))
            .join("include");
    h::generate(&include.join("mhf_mod.h"))?;
    h::write(
        &include.join("mhf_game.h"),
        "MHF_GAME_H",
        Some("mhf_mod.h"),
        h::definitions()?,
        |definer| {
            mhf_config::define_header(definer)?;
            mhf_font::api::define_header(definer)?;
            mhf_quest::api::define_header(definer)?;
            mhf_debug_tools::api::define_header(definer)?;
            mhf_ui::api::define_header(definer)
        },
    )?;
    mhf_mod_host::data::generate_header(&include.join("mhf_data.h"))
}
