use mhf_resource::{PathSegment, ResourcePath};

#[test]
fn format_paths_preserve_original_indices_and_unknown_schema_fields() {
    for (text, expected) in [
        (
            "motion/w04.mot#4/5",
            vec![PathSegment::Index(4), PathSegment::Index(5)],
        ),
        (
            "mhfsdt.bin#0/attacks/23",
            vec![
                PathSegment::Index(0),
                PathSegment::Field("attacks".into()),
                PathSegment::Index(23),
            ],
        ),
        (
            "mhfemd.bin#17/42",
            vec![PathSegment::Index(17), PathSegment::Index(42)],
        ),
        (
            "unknown.bin#future_table/4294967295/raw_08",
            vec![
                PathSegment::Field("future_table".into()),
                PathSegment::Index(u32::MAX),
                PathSegment::Field("raw_08".into()),
            ],
        ),
    ] {
        let path: ResourcePath = text.parse().unwrap();
        assert_eq!(path.segments(), expected);
        assert_eq!(path.to_string(), text);
        assert_eq!(path.to_string().parse::<ResourcePath>().unwrap(), path);
    }
}

#[test]
fn escaped_sources_preserve_unicode_case_and_literal_delimiters() {
    let source = "motion\\怪物 A#100%.mot";
    let path =
        ResourcePath::from_parts(source, [PathSegment::Index(4), PathSegment::Index(5)]).unwrap();
    assert_eq!(path.source(), "motion/怪物 A#100%.mot");
    assert_eq!(path.to_string(), "motion/怪物 A%23100%25.mot#4/5");
    assert_eq!(path.to_string().parse::<ResourcePath>().unwrap(), path);

    let encoded: ResourcePath = "motion/%E6%80%AA%E7%89%A9%20A%23100%25.mot#004/005"
        .parse()
        .unwrap();
    assert_eq!(encoded, path);
    assert_eq!(
        "A%2523.bin".parse::<ResourcePath>().unwrap().source(),
        "A%23.bin"
    );
    assert_eq!(
        "a%23b.bin".parse::<ResourcePath>().unwrap().to_string(),
        "a%23b.bin"
    );
}

#[test]
fn indices_normalize_decimal_text_without_guessing_numeric_field_names() {
    let path: ResourcePath = "motion\\W04.mot#00004/0005/Unknown_v2".parse().unwrap();
    assert_eq!(path.to_string(), "motion/W04.mot#4/5/Unknown_v2");
    assert_eq!(path.segments()[2], PathSegment::Field("Unknown_v2".into()));
    assert_eq!(
        "file.bin#00000000000000000000"
            .parse::<ResourcePath>()
            .unwrap()
            .to_string(),
        "file.bin#0"
    );
}

#[test]
fn malformed_addresses_do_not_become_other_resource_identities() {
    for text in [
        "",
        "#0",
        "file.bin#",
        "file.bin#/0",
        "file.bin#0/",
        "file.bin#0//1",
        "file.bin#0#1",
        "file.bin#4294967296",
        "file.bin#-1",
        "file.bin#+1",
        "file.bin#0x10",
        "file.bin#12abc",
        "file.bin#中文",
        "file.bin#field-name",
        "file.bin#attacks%2F23",
        "file.bin# field",
        "file.bin#field.name",
        "/file.bin",
        "C:/file.bin",
        "C:file.bin",
        "\\\\server\\file.bin",
        "a//file.bin",
        "a/./file.bin",
        "a/../file.bin",
        "file.bin/",
        "./file.bin",
        "../file.bin",
        "a\\..\\file.bin",
        "file%",
        "file%2",
        "file%XZ.bin",
        "file%FF.bin",
        "file%00.bin",
        "file\n.bin",
    ] {
        assert!(text.parse::<ResourcePath>().is_err(), "accepted {text:?}");
    }
}

#[test]
fn construction_and_mutation_keep_the_same_typed_parts() {
    let mut path = ResourcePath::new("mhfsdt.bin").unwrap();
    assert_eq!(path.to_string(), "mhfsdt.bin");
    let segments = [
        PathSegment::Index(0),
        PathSegment::Field("attacks".into()),
        PathSegment::Index(23),
    ];
    for segment in &segments {
        path.push(segment.clone()).unwrap();
    }
    assert_eq!(
        path,
        ResourcePath::from_parts("mhfsdt.bin", segments.clone()).unwrap()
    );
    let before = path.clone();
    assert!(path.push(PathSegment::Field("23".into())).is_err());
    assert_eq!(path, before);
    assert_eq!(
        ResourcePath::from_parts(path.source(), path.segments().iter().cloned()).unwrap(),
        path
    );
    assert!(ResourcePath::from_parts("file.bin", [PathSegment::Field("".into())]).is_err());
}
