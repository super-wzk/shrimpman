use super::{RecordCount, RecordFormat, TableLayout};
use crate::effect::{
    AttachmentDefinition, AttachmentGroup, ModelEffectBinding, ModelEffectDefinition,
};

#[derive(Clone, Copy, Debug)]
pub enum EffectRecordKind {
    AttachmentGroup,
    AttachmentDefinition,
    ModelBinding,
    ModelDefinition,
}

// Root +0x10 points to the u16 count block. These four count fields delimit
// the physical records (including reserved index 0 and any counted sentinel).
// Runtime 10BBA300 additionally stops matching entry 165 at part_code == 0,
// starting at index 1. It must not renumber either definition-ID namespace.
pub static EFFECT_TABLES: &[TableLayout] = &[
    TableLayout {
        id: "attachment_groups",
        label: "附着特效绑定（160）",
        root: &[0x280],
        first_record: 0,
        records: RecordCount::U16(&[0x10, 0x72]),
        stride: AttachmentGroup::SIZE as u16,
        format: RecordFormat::Effect(EffectRecordKind::AttachmentGroup),
        directory: None,
        names: None,
    },
    TableLayout {
        id: "attachment_definitions",
        label: "附着特效定义（161）",
        root: &[0x284],
        first_record: 0,
        records: RecordCount::U16(&[0x10, 0x74]),
        stride: AttachmentDefinition::SIZE as u16,
        format: RecordFormat::Effect(EffectRecordKind::AttachmentDefinition),
        directory: None,
        names: None,
    },
    TableLayout {
        id: "model_effect_bindings",
        label: "模型特效绑定（165）",
        root: &[0x294],
        first_record: 0,
        records: RecordCount::U16(&[0x10, 0x7c]),
        stride: ModelEffectBinding::SIZE as u16,
        format: RecordFormat::Effect(EffectRecordKind::ModelBinding),
        directory: None,
        names: None,
    },
    TableLayout {
        id: "model_effect_definitions",
        label: "模型特效定义（166）",
        root: &[0x298],
        first_record: 0,
        records: RecordCount::U16(&[0x10, 0x7e]),
        stride: ModelEffectDefinition::SIZE as u16,
        format: RecordFormat::Effect(EffectRecordKind::ModelDefinition),
        directory: None,
        names: None,
    },
];
