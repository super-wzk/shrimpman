fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=assets/icon.rc");
    println!("cargo::rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(
            "assets/icon.rc",
            embed_resource::ParamsIncludeDirs(["assets"]),
        )
        .manifest_required()
        .expect("failed to embed the mod manager icon");
    }
}
