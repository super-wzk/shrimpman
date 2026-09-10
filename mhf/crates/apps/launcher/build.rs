//! Embed the application icon and aggregate the public domain contracts.

fn main() -> std::io::Result<()> {
    use mhf_mod_api::headers as h;
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=assets/icon.rc");
    println!("cargo::rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(
            "assets/icon.rc",
            embed_resource::ParamsIncludeDirs(["assets"]),
        )
        .manifest_required()
        .expect("failed to embed the launcher icon");
    }
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
