use super::{Builder, Emd, Kind, ROOT_LABELS, relations};
use mhf_resource::PathSegment::Index;

impl Builder {
    pub(super) fn emd_global_table_node(
        &mut self,
        parent: usize,
        file: &Emd<'_>,
        base: usize,
        slot: usize,
    ) {
        let related = relations::global_records(file, slot);
        let range = file
            .root_table(slot)
            .ok()
            .flatten()
            .map_or(slot * 4..slot * 4 + 4, |table| table.range);
        let Some(node) = self.child(
            parent,
            format!("{slot:02} · {}", ROOT_LABELS[slot]),
            Kind::EmdGlobalTable(slot),
            self.document.nodes[parent].buffer,
            base + range.start..base + range.end,
        ) else {
            return;
        };
        if let Ok(root) = self.emd_root(node) {
            self.set_address(node, root, [Index(slot as u32)]);
        }
        self.field(node, "记录数", related.records.len(), base + slot * 4, 0);
        self.document.nodes[node].deferred = !related.records.is_empty();
        for error in related.errors {
            let message = match error.directory {
                Some(group) => format!("组 {group:03}：{}", error.message),
                None => error.message,
            };
            self.fail(node, message);
        }
    }

    pub(in crate::inspect) fn emd_global_records(
        &mut self,
        node: usize,
        slot: usize,
    ) -> Result<(), String> {
        let root = &self.document.nodes[self.emd_root(node)?];
        let buffer = self.document.buffers[root.buffer].clone();
        let base = root.range.start;
        let file = Emd::parse(&buffer[root.range.clone()]).map_err(|error| error.to_string())?;
        for reference in relations::global_records(&file, slot).records {
            let label = if slot == 19 && reference.record == 0 {
                "默认记录 000".to_owned()
            } else if let Some(group) = reference.directory {
                format!("组 {group:03} · 记录 {:03}", reference.record)
            } else {
                format!("记录 {:03}", reference.record)
            };
            if let Err(error) = self.emd_record_node(node, &file, base, reference, &label) {
                self.fail(node, format!("{label}：{error}"));
            }
        }
        Ok(())
    }
}
