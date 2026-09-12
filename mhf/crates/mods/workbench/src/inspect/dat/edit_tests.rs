use super::*;
use crate::{
    edit,
    field::Patch,
    inspect::{Document, expand, inspect},
};
use mhf_resource::{
    container::open_layers,
    effect::{AttachmentDefinition, ModelEffectDefinition},
};

fn expand_record(document: Document, table: usize, record: usize) -> (Document, usize) {
    let table = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::DatTable(table))
        .unwrap();
    let document = expand(&document, table).unwrap();
    let record = document.nodes[table].children[record];
    (expand(&document, record).unwrap(), record)
}

#[test]
#[ignore = "requires MHF_RESOURCE_GAME_ROOT; verifies typed edits against a complete original mhfdat"]
fn original_dat_typed_fields_share_reader_bindings_and_repack_without_unrelated_changes() {
    let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
    let source = std::fs::read(root.join("dat/mhfdat.bin")).unwrap();
    let document = inspect("mhfdat.bin", source.into());
    let (document, attachment) = expand_record(document, dat::DATA_TABLES.len() + 1, 1);
    let (document, model) = expand_record(document, dat::DATA_TABLES.len() + 3, 1);
    let (document, item) = expand_record(document, 7, 1);
    let dat_node = document
        .nodes
        .iter()
        .position(|node| node.kind == Kind::Dat)
        .unwrap();
    let mut expected = document.bytes(dat_node).unwrap().to_vec();
    let root_start = document.nodes[dat_node].range.start;
    let changes = [
        (attachment, "局部位置 XYZ", "12.5, -0, -3.25"),
        (attachment, "统一缩放 · 起始值", "1.75"),
        (attachment, "旋转 X · 起始角度（度）", "-123"),
        (attachment, "颜色 · 起始 RGB", "17, 128, 254"),
        (attachment, "渲染标志", "0xA5"),
        (model, "节点位移增量 XYZ", "-8, 0.25, 16"),
        (model, "缩放 X · 起始值", "-0"),
        (model, "颜色 · 结束 RGB", "255, 0, 91"),
        (model, "显隐条件标志", "0x41"),
        (item, "名称", "テスト"),
        (item, "买入价格", "4294967295"),
        (item, "稀有度原值", "255"),
    ];
    let mut patches: Vec<Patch> = Vec::new();
    for (node, name, input) in changes {
        let field = document.nodes[node]
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap();
        assert!(field.writable, "{name}");
        let canonical = field.read(&document.buffers).unwrap();
        assert!(
            field
                .write(&document.buffers, &canonical)
                .unwrap()
                .is_none(),
            "{name}"
        );
        if let Some(patch) = field.write(&document.buffers, input).unwrap() {
            let range =
                patch.binding.range.start - root_start..patch.binding.range.end - root_start;
            expected[range].copy_from_slice(&patch.after);
            patches.push(patch);
        }
    }
    let keys = [attachment, model, item].map(|node| edit::node_key(&document, node).unwrap());
    let updated = edit::apply_many(&document, &patches).unwrap();
    let [attachment, model, item] = keys.map(|key| edit::locate(&updated, &key).unwrap());
    assert!(updated.nodes.iter().all(|node| node.error.is_none()));
    let decoded = open_layers(&updated.buffers[0], usize::MAX, 10).unwrap();
    assert_eq!(decoded.payload(), expected);

    // Independent domain parsers consume the rebuilt bytes, including every
    // sign bit, color channel, flag and value beyond signed integer limits.
    let attachment_record =
        AttachmentDefinition::parse(updated.bytes(attachment).unwrap()).unwrap();
    assert_eq!(
        attachment_record.local_position_bits,
        [
            12.5_f32.to_bits(),
            (-0_f32).to_bits(),
            (-3.25_f32).to_bits()
        ]
    );
    assert_eq!(attachment_record.scale.start_bits, 1.75_f32.to_bits());
    assert_eq!(attachment_record.rotation[0].start_degrees, -123);
    assert_eq!(attachment_record.color.start_rgb, [17, 128, 254]);
    assert_eq!(attachment_record.render_flags, 0xa5);
    let model_record = ModelEffectDefinition::parse(updated.bytes(model).unwrap()).unwrap();
    assert_eq!(
        model_record.translation_delta_bits,
        [(-8_f32).to_bits(), 0.25_f32.to_bits(), 16_f32.to_bits()]
    );
    assert_eq!(model_record.scale[0].start_bits, (-0_f32).to_bits());
    assert_eq!(model_record.color.end_rgb, [255, 0, 91]);
    assert_eq!(model_record.visibility_flags, 0x41);
    let name = updated.nodes[item]
        .fields
        .iter()
        .find(|field| field.name == "名称")
        .unwrap();
    assert_eq!(name.read(&updated.buffers).unwrap(), "テスト");
    assert!(updated.nodes[item].name.contains("テスト"));
    assert_eq!(
        &name.binding.bytes(&updated.buffers).unwrap()[..7],
        &[0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0]
    );
    assert_eq!(
        u32::from_le_bytes(updated.bytes(item).unwrap()[12..16].try_into().unwrap()),
        u32::MAX
    );
    assert_eq!(updated.bytes(item).unwrap()[2], u8::MAX);
    eprintln!(
        "{} typed patches round-tripped through the original complete DAT",
        patches.len()
    );
}
