//! The native DAT motion directories share headers but have distinct leaves.

use super::{Builder, Dat, Kind};
use mhf_resource::PathSegment::{Field as Key, Index};
use mhf_resource::dat::motion_events::{Directory, EventKind};

impl Builder {
    pub(in crate::inspect) fn dat_motion_event_directories(
        &mut self,
        node: usize,
        file: &Dat<'_>,
        base: usize,
    ) {
        let buffer = self.document.nodes[node].buffer;
        for (kind, name) in [
            (EventKind::Command, "动作操作事件（390）"),
            (EventKind::Choice, "动作分派事件（391）"),
        ] {
            let field = kind.root() as usize * 4;
            if file.u32(field).is_ok_and(|pointer| pointer == 0) {
                continue;
            }
            let directory = Directory::parse(file.as_bytes(), 0, kind);
            let range = directory
                .as_ref()
                .map_or(field..field + 4, |directory| directory.range.clone());
            let Some(parent) = self.child(
                node,
                name,
                Kind::Block,
                buffer,
                base + range.start..base + range.end,
            ) else {
                return;
            };
            self.set_address(parent, node, [Index(kind.root())]);
            let directory = match directory {
                Ok(directory) => directory,
                Err(error) => {
                    self.fail(parent, error.to_string());
                    continue;
                }
            };
            self.field(parent, "分组数", directory.count, base + field, 0);
            for group in 0..directory.count {
                let at = base + directory.range.start + group * 8;
                let Some(child) = self.child(
                    parent,
                    format!("分组[{group}]"),
                    Kind::DatMotionEventGroup(kind, group),
                    buffer,
                    at..at + 8,
                ) else {
                    break;
                };
                self.set_address(child, parent, [Index(group as u32)]);
                let result = self
                    .read::<u16>(child, "count", at)
                    .and_then(|_| self.read::<u16>(child, "unknown_02", at + 2))
                    .and_then(|_| self.read::<u32>(child, "records_offset", at + 4));
                if let Err(error) = result {
                    self.fail(child, error);
                    continue;
                }
                match directory.group(group) {
                    Ok(group) => self.document.nodes[child].deferred = group.count != 0,
                    Err(error) => self.fail(child, error.to_string()),
                }
            }
        }
    }

    pub(in crate::inspect) fn dat_motion_event_group(
        &mut self,
        node: usize,
        kind: EventKind,
        group: usize,
    ) -> Result<(), String> {
        let owner = self.dat_owner(node)?;
        let root = &self.document.nodes[owner];
        let buffer_index = root.buffer;
        let buffer = self.document.buffers[buffer_index].clone();
        let base = root.range.start;
        let directory = Directory::parse(&buffer[root.range.clone()], 0, kind)
            .map_err(|error| error.to_string())?;
        let group_header = directory.group(group).map_err(|error| error.to_string())?;
        for index in 0..usize::from(group_header.count) {
            let at = base + group_header.records.start + index * 8;
            let Some(child) = self.child(
                node,
                format!("条目[{index}]"),
                Kind::DatMotionEventEntry(kind, group, index),
                buffer_index,
                at..at + 8,
            ) else {
                break;
            };
            self.set_address(child, node, [Index(index as u32)]);
            self.read::<u16>(child, "key", at)?;
            self.read::<u16>(child, "count", at + 2)?;
            self.read::<u32>(child, "events_offset", at + 4)?;
            match directory.entry(group, index) {
                Ok(entry) => self.document.nodes[child].deferred = entry.count != 0,
                Err(error) => self.fail(child, error.to_string()),
            }
        }
        Ok(())
    }

    pub(in crate::inspect) fn dat_motion_event_entry(
        &mut self,
        node: usize,
        kind: EventKind,
        group: usize,
        index: usize,
    ) -> Result<(), String> {
        let owner = self.dat_owner(node)?;
        let root = &self.document.nodes[owner];
        let buffer_index = root.buffer;
        let buffer = self.document.buffers[buffer_index].clone();
        let base = root.range.start;
        let directory = Directory::parse(&buffer[root.range.clone()], 0, kind)
            .map_err(|error| error.to_string())?;
        let entry = directory
            .entry(group, index)
            .map_err(|error| error.to_string())?;
        let Some(events) = self.child(
            node,
            "事件",
            Kind::Block,
            buffer_index,
            base + entry.events.start..base + entry.events.end,
        ) else {
            return Ok(());
        };
        self.set_address(events, node, [Key("events".into())]);
        for event in 0..usize::from(entry.count) {
            let at = base + entry.events.start + event * kind.record_size();
            let Some(child) = self.child(
                events,
                format!("事件[{event}]"),
                Kind::Block,
                buffer_index,
                at..at + kind.record_size(),
            ) else {
                break;
            };
            self.set_address(child, events, [Index(event as u32)]);
            self.read::<u16>(child, "frame", at)?;
            match kind {
                EventKind::Command => {
                    for (offset, key) in [(2, "operation"), (4, "arg_a"), (6, "arg_b")] {
                        self.read::<u16>(child, key, at + offset)?;
                    }
                    self.read::<u8>(child, "arg_c", at + 8)?;
                    self.read::<u8>(child, "arg_d", at + 9)?;
                    for (offset, key) in [(10, "arg_e"), (12, "arg_f"), (14, "arg_g")] {
                        self.read::<u16>(child, key, at + offset)?;
                    }
                }
                EventKind::Choice => {
                    self.read::<u16>(child, "dispatch_kind", at + 2)?;
                    self.read::<u8>(child, "condition", at + 4)?;
                    self.read::<u8>(child, "unknown_05", at + 5)?;
                    for choice in 0..4 {
                        let choice_at = at + 6 + choice * 4;
                        let Some(option) = self.child(
                            child,
                            format!("选项[{choice}]"),
                            Kind::Block,
                            buffer_index,
                            choice_at..choice_at + 4,
                        ) else {
                            break;
                        };
                        self.set_address(option, child, [Key(format!("choice_{choice}"))]);
                        self.read::<u16>(option, "id", choice_at)?;
                        self.read::<u16>(option, "weight", choice_at + 2)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
