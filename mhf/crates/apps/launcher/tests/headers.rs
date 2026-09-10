#[path = "../../../../build/header_snapshot.rs"]
mod header_snapshot;

#[test]
fn published_headers_match_build() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    header_snapshot::check_headers(&[
        (
            "mhf_mod.h",
            include_bytes!(concat!(env!("OUT_DIR"), "/include/mhf_mod.h")),
            root.join("../../runtime/mod-api/include/mhf_mod.h"),
        ),
        (
            "mhf_game.h",
            include_bytes!(concat!(env!("OUT_DIR"), "/include/mhf_game.h")),
            root.join("include/mhf_game.h"),
        ),
        (
            "mhf_data.h",
            include_bytes!(concat!(env!("OUT_DIR"), "/include/mhf_data.h")),
            root.join("../../runtime/mod-host/include/mhf_data.h"),
        ),
    ]);
}
