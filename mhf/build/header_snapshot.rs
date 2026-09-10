//! Cargo integration-test support for published C header snapshots.

use std::{env, fs, path::PathBuf};

pub fn check_headers(headers: &[(&str, &[u8], PathBuf)]) {
    let update = env::var("MHF_UPDATE_HEADERS").as_deref() == Ok("1");
    let mut stale = Vec::new();
    for (name, generated, published) in headers {
        if fs::read(published).ok().as_deref() == Some(*generated) {
            continue;
        }
        if update {
            fs::create_dir_all(published.parent().unwrap()).unwrap();
            fs::write(published, generated).unwrap();
            println!("updated {}", published.display());
        } else {
            stale.push(*name);
        }
    }
    assert!(
        stale.is_empty(),
        "C headers differ from the API definitions: {}. Run this test with MHF_UPDATE_HEADERS=1 to refresh the published snapshots.",
        stale.join(", ")
    );
    if let Some(directory) = env::var_os("MHF_HEADERS_EXPORT_DIR") {
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory).unwrap();
        for (name, generated, _) in headers {
            let destination = directory.join(name);
            fs::write(&destination, generated).unwrap();
            println!("exported {}", destination.display());
        }
    }
}
