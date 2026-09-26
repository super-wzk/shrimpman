use super::*;
#[test]
fn resource_lengths_follow_address_space_instead_of_a_fixed_file_limit() {
    let length = 256 * 1024 * 1024 + 1;
    assert_eq!(file_length(length).unwrap(), length as usize);
    assert_eq!(file_length(isize::MAX as u64).unwrap(), isize::MAX as usize);
    assert!(file_length(isize::MAX as u64 + 1).is_err());
    assert!(file_length(u64::MAX).is_err());
}

#[test]
fn resource_reads_use_actual_bytes_when_metadata_length_changes() {
    let source: Vec<_> = (0..131_079).map(|index| index as u8).collect();
    for length in [0, 17, source.len() as u64, source.len() as u64 + 100] {
        assert_eq!(
            read_resource(io::Cursor::new(&source), length).unwrap(),
            source
        );
    }
}

#[test]
fn packing_replaces_the_corresponding_override_and_preserves_the_source() {
    let directory = std::env::temp_dir().join(format!("mhf-workbench-pack-{}", std::process::id()));
    let data = directory.join("dat");
    let output = directory.join("dat-redirect");
    fs::create_dir_all(data.join("nested")).unwrap();
    let source = data.join("nested/mhfdat.bin");
    fs::write(&source, [1, 2, 3]).unwrap();
    let target = pack_bytes(&data, &output, &source, &[4, 5, 6]).unwrap();
    assert_eq!(target, output.join("nested/mhfdat.bin"));
    pack_bytes(&data, &output, &source, &[7, 8]).unwrap();
    assert_eq!(fs::read(&target).unwrap(), [7, 8]);
    assert_eq!(fs::read(&source).unwrap(), [1, 2, 3]);
    assert!(pack_bytes(&data, &data, &source, &[9]).is_err());
    assert!(pack_bytes(&data, &output, &data.join("../outside.bin"), &[9]).is_err());
    assert_eq!(fs::read_dir(target.parent().unwrap()).unwrap().count(), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn packing_rejects_symlinked_subdirectories_outside_the_override_root() {
    let directory =
        std::env::temp_dir().join(format!("mhf-workbench-pack-link-{}", std::process::id()));
    let data = directory.join("dat");
    let output = directory.join("redirect");
    let outside = directory.join("outside");
    for path in [&data, &output, &outside] {
        fs::create_dir_all(path).unwrap();
    }
    std::os::unix::fs::symlink(&outside, output.join("nested")).unwrap();
    assert!(pack_bytes(&data, &output, &data.join("nested/a.bin"), &[9]).is_err());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_dir_all(directory).unwrap();
}
