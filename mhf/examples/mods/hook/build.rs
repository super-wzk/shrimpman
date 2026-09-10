use std::{env, path::PathBuf};

#[path = "src/probe.rs"]
mod probe;

fn main() -> std::io::Result<()> {
    use mhf_mod_sdk::abi::headers as h;
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/probe.rs");
    let include_dir =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("include");
    h::generate(&include_dir.join("mhf_mod.h"))?;
    h::write(
        &include_dir.join("probe.h"),
        "EXAMPLE_HOOK_PROBE_H",
        Some("mhf_mod.h"),
        h::definitions()?,
        |d| {
            h::alias::<probe::HookSnapshotV1>(d, "HookSnapshotV1")?;
            h::alias::<probe::HookProbeV1>(d, "HookProbeV1")?;
            h::string(d, "EXAMPLE_HOOK_PROVIDER", probe::PROVIDER_ID)?;
            h::string(d, "EXAMPLE_HOOK_INTERFACE", probe::INTERFACE_ID)
        },
    )
}
