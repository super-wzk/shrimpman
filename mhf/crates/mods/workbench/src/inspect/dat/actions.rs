//! DAT weapon actions use the same native records as the runtime debugger.

use super::{Builder, Dat, Kind};
use crate::field::FieldReference;
use mhf_resource::PathSegment::{Field as Key, Index};
use mhf_resource::action_definition::{
    self, ActionCondition, ActionTransition, Definition, NativeMotionRef,
};

impl Builder {
    pub(in crate::inspect) fn dat_action_directory(
        &mut self,
        node: usize,
        file: &Dat<'_>,
        base: usize,
    ) {
        let field = action_definition::DAT_ROOT as usize * 4;
        let directory = match file.pointer(field) {
            Ok(Some(directory)) => directory,
            Ok(None) => return,
            Err(error) => {
                self.fail(node, error.to_string());
                return;
            }
        };
        let buffer = self.document.nodes[node].buffer;
        let length = usize::from(action_definition::WEAPON_COUNT) * 8;
        if let Err(error) = file.bytes(directory, length) {
            self.fail(node, error.to_string());
            return;
        }
        let Some(parent) = self.child(
            node,
            "武器招式定义",
            Kind::Block,
            buffer,
            base + directory..base + directory + length,
        ) else {
            return;
        };
        self.set_address(parent, node, [Index(action_definition::DAT_ROOT)]);
        for weapon in 0..action_definition::WEAPON_COUNT {
            let at = base + directory + usize::from(weapon) * 8;
            let Some(child) = self.child(
                parent,
                format!("武器[{weapon}]"),
                Kind::DatWeaponActions(weapon),
                buffer,
                at..at + 8,
            ) else {
                break;
            };
            self.set_address(child, parent, [Index(u32::from(weapon))]);
            match action_definition::weapon_actions(file.as_bytes(), 0, weapon) {
                Ok(actions) => {
                    let result = self
                        .read::<u32>(child, "count", at)
                        .and_then(|_| self.read::<u32>(child, "records_offset", at + 4));
                    if let Err(error) = result {
                        self.fail(child, error);
                    }
                    self.document.nodes[child].deferred = actions.count != 0;
                }
                Err(error) => self.fail(child, error.to_string()),
            }
        }
    }

    pub(in crate::inspect) fn dat_weapon_actions(
        &mut self,
        node: usize,
        weapon: u8,
    ) -> Result<(), String> {
        let owner = self.dat_owner(node)?;
        let root = &self.document.nodes[owner];
        let buffer_index = root.buffer;
        let buffer = self.document.buffers[buffer_index].clone();
        let base = root.range.start;
        let directory = action_definition::weapon_actions(&buffer[root.range.clone()], 0, weapon)
            .map_err(|error| error.to_string())?;
        for action in 0..directory.count {
            let at =
                base + directory.records.start + action as usize * action_definition::ACTION_SIZE;
            let Some(child) = self.child(
                node,
                format!("招式[{action}]"),
                Kind::DatAction(weapon, action as u16),
                buffer_index,
                at..at + action_definition::ACTION_SIZE,
            ) else {
                break;
            };
            self.set_address(child, node, [Index(action)]);
            for (offset, key) in [
                (0, "step_count"),
                (4, "steps_offset"),
                (8, "transition_count"),
                (12, "transitions_offset"),
                (16, "event_count"),
                (20, "events_offset"),
            ] {
                self.read::<u32>(child, key, at + offset)?;
            }
            self.document.nodes[child].deferred = true;
        }
        Ok(())
    }

    pub(in crate::inspect) fn dat_action_contents(
        &mut self,
        node: usize,
        weapon: u8,
        action: u16,
    ) -> Result<(), String> {
        let owner = self.dat_owner(node)?;
        let root = &self.document.nodes[owner];
        let buffer_index = root.buffer;
        let buffer = self.document.buffers[buffer_index].clone();
        let base = root.range.start;
        let definition = Definition::parse(&buffer[root.range.clone()], 0, weapon, action)
            .map_err(|error| error.to_string())?;
        let Some(steps) = self.child(
            node,
            "动作步骤",
            Kind::Block,
            buffer_index,
            base + definition.steps_range.start..base + definition.steps_range.end,
        ) else {
            return Ok(());
        };
        self.set_address(steps, node, [Key("steps".into())]);
        for (index, step) in definition.steps.iter().enumerate() {
            let span = definition
                .step_span(index)
                .expect("decoded step has a source span");
            let at = base + span.start;
            let Some(child) = self.child(
                steps,
                format!("步骤[{index}]"),
                Kind::Block,
                buffer_index,
                at..base + span.end,
            ) else {
                break;
            };
            self.set_address(child, steps, [Index(index as u32)]);
            for (word, key) in ["kind", "value", "arg_a", "arg_b", "frame", "count"]
                .into_iter()
                .enumerate()
            {
                self.read::<u16>(child, key, at + word * 2)?;
            }
            if !matches!(step.0[0], 0..=2) {
                let reference = NativeMotionRef {
                    id: step.0[1],
                    weapon,
                    style: None,
                };
                let field = self.document.nodes[child].fields.len();
                self.field(child, "动画引用", "", at + 2, 0);
                self.document.nodes[child].fields[field].reference =
                    Some(FieldReference::Motion(reference));
            }
        }
        let Some(transitions) = self.child(
            node,
            "招式派生条件",
            Kind::Block,
            buffer_index,
            base + definition.transitions_range.start..base + definition.transitions_range.end,
        ) else {
            return Ok(());
        };
        self.set_address(transitions, node, [Key("transitions".into())]);
        for (index, offset) in definition
            .transitions_range
            .clone()
            .step_by(ActionTransition::SIZE)
            .enumerate()
        {
            let at = base + offset;
            let Some(child) = self.child(
                transitions,
                format!("派生条件[{index}]"),
                Kind::Block,
                buffer_index,
                at..at + ActionTransition::SIZE,
            ) else {
                break;
            };
            self.set_address(child, transitions, [Index(index as u32)]);
            for (offset, key) in [
                (0, "priority"),
                (2, "input"),
                (4, "selection"),
                (22, "argument"),
            ] {
                self.read::<u16>(child, key, at + offset)?;
            }
            for (offset, key, name) in [
                (6, "input_start", "输入起始条件"),
                (14, "input_end", "输入结束条件"),
                (24, "transition_start", "派生起始条件"),
                (32, "transition_end", "派生结束条件"),
            ] {
                let condition_at = at + offset;
                let Some(condition) = self.child(
                    child,
                    name,
                    Kind::Block,
                    buffer_index,
                    condition_at..condition_at + ActionCondition::SIZE,
                ) else {
                    break;
                };
                self.set_address(condition, child, [Key(key.into())]);
                self.read::<u16>(condition, "step", condition_at)?;
                self.read::<u8>(condition, "timing", condition_at + 2)?;
                self.read::<i8>(condition, "phase", condition_at + 3)?;
                self.read::<u16>(condition, "frame", condition_at + 4)?;
                self.read::<u16>(condition, "count", condition_at + 6)?;
            }
        }
        let Some(events) = self.child(
            node,
            "招式事件",
            Kind::Block,
            buffer_index,
            base + definition.events_range.start..base + definition.events_range.end,
        ) else {
            return Ok(());
        };
        self.set_address(events, node, [Key("events".into())]);
        for (index, event) in definition.events.iter().enumerate() {
            let span = definition
                .event_span(index)
                .expect("decoded event has a source span");
            let at = base + span.start;
            let Some(child) = self.child(
                events,
                format!("事件[{index}] · 步骤[{}]", event.step),
                Kind::Block,
                buffer_index,
                at..base + span.end,
            ) else {
                break;
            };
            self.set_address(child, events, [Index(index as u32)]);
            self.read::<u16>(child, "step", at)?;
            self.read::<u8>(child, "timing", at + 2)?;
            self.read::<i8>(child, "phase", at + 3)?;
            for (offset, key) in [
                (4, "frame"),
                (6, "count"),
                (8, "operation"),
                (10, "argument"),
            ] {
                self.read::<u16>(child, key, at + offset)?;
            }
            if let Some(reference) = event.attack_reference(weapon) {
                let field = self.document.nodes[child].fields.len();
                self.field(child, "攻击资源", "", at + 10, 0);
                self.document.nodes[child].fields[field].reference =
                    Some(FieldReference::Attack(reference));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        edit,
        field::{FieldType, ScalarType},
        inspect::{self, resource_path::Location},
    };
    use mhf_resource::{
        ResourcePath,
        action_definition::{ActionEvent, ActionStep},
    };
    use std::path::Path;

    fn dword(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn native_event_paths_locate_original_rows_and_survive_labels_and_byte_edits() {
        let directory = mhf_resource::dat::HEADER_SIZE;
        let action = directory + usize::from(action_definition::WEAPON_COUNT) * 8;
        let step = action + action_definition::ACTION_SIZE;
        let events = step + ActionStep::SIZE;
        let mut bytes = vec![0; events + ActionEvent::SIZE * 2];
        bytes[..4].copy_from_slice(mhf_resource::dat::MAGIC);
        dword(&mut bytes, 4, mhf_resource::dat::VERSION);
        dword(&mut bytes, 12, mhf_resource::dat::HEADER_SIZE as u32);
        dword(
            &mut bytes,
            action_definition::DAT_ROOT as usize * 4,
            directory as u32,
        );
        dword(&mut bytes, directory + 4 * 8, 1);
        dword(&mut bytes, directory + 4 * 8 + 4, action as u32);
        for (offset, value) in [
            (0, 1),
            (4, step as u32),
            (8, 0),
            (12, 0),
            (16, 2),
            (20, events as u32),
        ] {
            dword(&mut bytes, action + offset, value);
        }
        bytes[step..events]
            .copy_from_slice(&ActionStep([4, 1405, 0xffff, 0x8000, 12, 2]).to_bytes());
        let known = ActionEvent {
            step: 0,
            timing: 1,
            phase: -2,
            frame: 17,
            count: 9,
            operation: 12,
            argument: 23,
        };
        let unknown = ActionEvent {
            step: 0,
            timing: 9,
            phase: 7,
            frame: 0xfffe,
            count: 0x1234,
            operation: 0xffff,
            argument: 2,
        };
        bytes[events..events + ActionEvent::SIZE].copy_from_slice(&known.to_bytes());
        bytes[events + ActionEvent::SIZE..].copy_from_slice(&unknown.to_bytes());
        let root = Path::new("/fixtures/dat");
        let path: ResourcePath = "mhfdat.bin#389/4/0/events/1/phase".parse().unwrap();
        let mut document = inspect::inspect(root.join("mhfdat.bin"), bytes.clone().into());
        let (node, context, field) = locate_field(&mut document, root, &path);
        let binding = document.nodes[node].fields[field].binding.clone();
        assert_eq!(
            binding.range,
            events + ActionEvent::SIZE + 3..events + ActionEvent::SIZE + 4
        );
        assert_eq!(
            document.nodes[node].fields[field]
                .read(&document.buffers)
                .unwrap(),
            "7"
        );
        document.nodes[node].name = "重命名事件".into();
        document.nodes[node].fields[field].name = "阶段显示名称".into();
        let address = document
            .resource_address(root, &context, Some(field))
            .unwrap();
        assert!(address.exact);
        assert_eq!(address.path, path);
        let edited = edit::apply(
            &document,
            binding.buffer,
            binding.range.clone(),
            &[(-3_i8) as u8],
        )
        .unwrap();
        assert_eq!(edited.source, document.source);
        let mut expected = bytes;
        expected[binding.range.start] = (-3_i8) as u8;
        assert_eq!(&*edited.buffers[0], expected);
        let definition = Definition::parse(&edited.buffers[0], 0, 4, 0).unwrap();
        assert_eq!(definition.events[0], known);
        assert_eq!(definition.events[1].phase, -3);
        assert_eq!(definition.events[1].operation, unknown.operation);
        assert_eq!(definition.events[1].frame, unknown.frame);
        assert_eq!(definition.events[1].count, unknown.count);
    }

    fn transition_image() -> (Vec<u8>, usize, usize, ActionTransition) {
        let directory = mhf_resource::dat::HEADER_SIZE;
        let action = directory + usize::from(action_definition::WEAPON_COUNT) * 8;
        let transitions = action + action_definition::ACTION_SIZE;
        let mut bytes = vec![0; transitions + ActionTransition::SIZE];
        bytes[..4].copy_from_slice(mhf_resource::dat::MAGIC);
        dword(&mut bytes, 4, mhf_resource::dat::VERSION);
        dword(&mut bytes, 12, mhf_resource::dat::HEADER_SIZE as u32);
        dword(
            &mut bytes,
            action_definition::DAT_ROOT as usize * 4,
            directory as u32,
        );
        dword(&mut bytes, directory + 4 * 8, 1);
        dword(&mut bytes, directory + 4 * 8 + 4, action as u32);
        dword(&mut bytes, action + 8, 1);
        dword(&mut bytes, action + 12, transitions as u32);
        let condition = |step, phase| ActionCondition {
            step,
            timing: 0xff,
            phase,
            frame: 0xfffe,
            count: 0xffff,
        };
        let transition = ActionTransition {
            priority: 0x8000,
            input: 0xffff,
            selection: 0xabcd,
            input_start: condition(0, -128),
            input_end: condition(1, -1),
            argument: 0x9876,
            transition_start: condition(2, 0),
            transition_end: condition(3, 127),
        };
        bytes[transitions..].copy_from_slice(&transition.to_bytes());
        (bytes, action, transitions, transition)
    }

    fn locate_field(
        document: &mut inspect::Document,
        root: &Path,
        path: &ResourcePath,
    ) -> (usize, Vec<usize>, usize) {
        loop {
            match document.locate_resource(root, path) {
                Location::Expand(node) => *document = inspect::expand(document, node).unwrap(),
                Location::Resolved {
                    node,
                    context,
                    field: Some(field),
                } => return (node, context, field),
                location => panic!("could not locate resource field {path}: {location:?}"),
            }
        }
    }

    #[test]
    fn transition_paths_keep_native_offsets_types_and_edits_inside_containers() {
        let (bytes, action, transitions, transition) = transition_image();
        let root = Path::new("/fixtures/dat");
        for base in [0, 32] {
            let mut source = vec![0; base];
            let prefix = if base == 0 {
                "mhfdat.bin#389/4/0"
            } else {
                dword(&mut source, 0, 1);
                dword(&mut source, 4, base as u32);
                dword(&mut source, 8, bytes.len() as u32);
                "mhfdat.bin#0/389/4/0"
            };
            source.extend_from_slice(&bytes);
            let mut document = inspect::inspect(root.join("mhfdat.bin"), source.clone().into());
            for (key, offset) in [("transition_count", 8), ("transitions_offset", 12)] {
                let path = format!("{prefix}/{key}").parse().unwrap();
                let (node, _, field) = locate_field(&mut document, root, &path);
                let binding = &document.nodes[node].fields[field].binding;
                assert_eq!(
                    binding.range,
                    base + action + offset..base + action + offset + 4
                );
                assert_eq!(binding.format, FieldType::Scalar(ScalarType::U32));
            }
            for (key, offset) in [
                ("priority", 0),
                ("input", 2),
                ("selection", 4),
                ("argument", 22),
            ] {
                let path = format!("{prefix}/transitions/0/{key}").parse().unwrap();
                let (node, _, field) = locate_field(&mut document, root, &path);
                let binding = &document.nodes[node].fields[field].binding;
                assert_eq!(
                    binding.range,
                    base + transitions + offset..base + transitions + offset + 2
                );
                assert_eq!(binding.format, FieldType::Scalar(ScalarType::U16));
            }
            for (condition, at) in [
                ("input_start", 6),
                ("input_end", 14),
                ("transition_start", 24),
                ("transition_end", 32),
            ] {
                for (key, offset, scalar, size) in [
                    ("step", 0, ScalarType::U16, 2),
                    ("timing", 2, ScalarType::U8, 1),
                    ("phase", 3, ScalarType::I8, 1),
                    ("frame", 4, ScalarType::U16, 2),
                    ("count", 6, ScalarType::U16, 2),
                ] {
                    let path = format!("{prefix}/transitions/0/{condition}/{key}")
                        .parse()
                        .unwrap();
                    let (node, context, field) = locate_field(&mut document, root, &path);
                    let binding = &document.nodes[node].fields[field].binding;
                    assert_eq!(
                        binding.range,
                        base + transitions + at + offset..base + transitions + at + offset + size
                    );
                    assert_eq!(binding.format, FieldType::Scalar(scalar));
                    let address = document
                        .resource_address(root, &context, Some(field))
                        .unwrap();
                    assert!(address.exact);
                    assert_eq!(address.path, path);
                }
            }
            let path = format!("{prefix}/transitions/0/input_start/phase")
                .parse()
                .unwrap();
            let (node, context, field) = locate_field(&mut document, root, &path);
            assert_eq!(
                document.nodes[node].fields[field]
                    .read(&document.buffers)
                    .unwrap(),
                "-128"
            );
            let binding = document.nodes[node].fields[field].binding.clone();
            document.nodes[node].name = "重命名条件".into();
            document.nodes[node].fields[field].name = "阶段显示名称".into();
            let address = document
                .resource_address(root, &context, Some(field))
                .unwrap();
            assert!(address.exact);
            assert_eq!(address.path, path);
            let edited_bytes = binding
                .format
                .encode(&source[binding.range.clone()], "-3")
                .unwrap();
            let edited = edit::apply(
                &document,
                binding.buffer,
                binding.range.clone(),
                &edited_bytes,
            )
            .unwrap();
            source[binding.range].copy_from_slice(&edited_bytes);
            assert_eq!(&*edited.buffers[0], source);
            let definition = Definition::parse(&edited.buffers[0][base..], 0, 4, 0).unwrap();
            let mut expected = transition;
            expected.input_start.phase = -3;
            assert_eq!(definition.transitions, [expected]);
        }
    }

    #[test]
    fn corrupt_weapon_does_not_hide_other_weapon_transitions() {
        let (mut bytes, _, _, _) = transition_image();
        let directory = mhf_resource::dat::HEADER_SIZE;
        dword(&mut bytes, directory + 3 * 8, 1);
        dword(&mut bytes, directory + 3 * 8 + 4, (directory - 4) as u32);
        let root = Path::new("/fixtures/dat");
        let mut document = inspect::inspect(root.join("mhfdat.bin"), bytes.into());
        let corrupt = document
            .nodes
            .iter()
            .find(|node| node.kind == Kind::DatWeaponActions(3))
            .unwrap();
        assert!(corrupt.error.is_some());
        assert!(!corrupt.deferred);
        assert!(corrupt.children.is_empty());
        let path = "mhfdat.bin#389/4/0/transitions/0/input".parse().unwrap();
        let (node, _, field) = locate_field(&mut document, root, &path);
        assert_eq!(
            document.nodes[node].fields[field]
                .read(&document.buffers)
                .unwrap(),
            "65535"
        );
    }

    #[test]
    fn truncated_weapon_directory_creates_no_out_of_range_nodes() {
        let (mut bytes, _, _, _) = transition_image();
        let truncated = bytes.len() - 8;
        dword(
            &mut bytes,
            action_definition::DAT_ROOT as usize * 4,
            truncated as u32,
        );
        let length = bytes.len();
        let document = inspect::inspect("mhfdat.bin", bytes.into());
        assert!(document.nodes[document.root].error.is_some());
        assert!(document.nodes.iter().all(|node| node.range.end <= length));
        assert!(
            !document
                .nodes
                .iter()
                .any(|node| matches!(node.kind, Kind::DatWeaponActions(_)))
        );
    }
}
