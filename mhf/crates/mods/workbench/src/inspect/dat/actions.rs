//! DAT weapon actions use the same native records as the runtime debugger.

use super::{Builder, Dat, Kind};
use crate::field::FieldReference;
use mhf_resource::PathSegment::{Field as Key, Index};
use mhf_resource::action_definition::{self, ActionEvent, ActionStep, Definition, NativeMotionRef};

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
                (8, "unknown_08"),
                (12, "unknown_0c"),
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
                at..at + ActionStep::SIZE,
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
                at..at + ActionEvent::SIZE,
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
        inspect::{self, resource_path::Location},
    };
    use mhf_resource::ResourcePath;
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
            (8, 0xabcd_1234),
            (12, 0x8765_4321),
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
        let (node, context, field) = loop {
            match document.locate_resource(root, &path) {
                Location::Expand(node) => document = inspect::expand(&document, node).unwrap(),
                Location::Resolved {
                    node,
                    context,
                    field: Some(field),
                } => break (node, context, field),
                location => panic!("could not locate original event: {location:?}"),
            }
        };
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
}
