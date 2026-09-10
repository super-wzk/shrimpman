use std::path::PathBuf;

#[path = "../../../../build/header_snapshot.rs"]
mod header_snapshot;

#[test]
fn headers() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    header_snapshot::check_headers(&[
        (
            "mhf_mod.h",
            include_bytes!(concat!(env!("OUT_DIR"), "/include/mhf_mod.h")),
            manifest.join("../../../crates/runtime/mod-api/include/mhf_mod.h"),
        ),
        (
            "counter.h",
            include_bytes!(concat!(env!("OUT_DIR"), "/include/counter.h")),
            manifest.join("../counter-sdk/counter.h"),
        ),
    ]);
}
