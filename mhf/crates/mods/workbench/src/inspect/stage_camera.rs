//! Context-selected scene camera regions, before native pointer relocation.

use super::{Builder, Kind, hex};
use mhf_resource::stage::AreaCamera;

impl Builder {
    pub(super) fn inspect_stage_area_camera(&mut self, node: usize, bytes: &[u8], base: usize) {
        self.document.nodes[node].kind = Kind::StageAreaCamera;
        match AreaCamera::parse(bytes) {
            Ok(file) => {
                for (offset, name) in [
                    (0, "版本"),
                    (2, "区域记录数"),
                    (4, "网格数 X"),
                    (6, "网格数 Z"),
                    (8, "网格尺寸 X"),
                    (10, "网格尺寸 Z"),
                ] {
                    let value = u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap());
                    self.field(node, name, value, base + offset, 2);
                }
                for offset in (12..48).step_by(4) {
                    let value = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
                    self.field(
                        node,
                        format!("word_{offset:02X}"),
                        format!("{value:#010X}"),
                        base + offset,
                        4,
                    );
                }
                self.document.nodes[node].deferred =
                    !file.regions.is_empty() || !file.cells.is_empty() || !file.trailing.is_empty();
            }
            Err(error) => self.fail(node, error.to_string()),
        }
    }

    pub(super) fn stage_area_camera_details(
        &mut self,
        node: usize,
        file: &AreaCamera<'_>,
        base: usize,
    ) {
        let buffer = self.document.nodes[node].buffer;
        for region in &file.regions {
            let bytes = region.as_bytes();
            let at = base + region.offset;
            let Some(child) = self.child(
                node,
                format!("相机区域 {}", region.index),
                Kind::Block,
                buffer,
                at..at + bytes.len(),
            ) else {
                break;
            };
            self.field(child, "扩展记录数", bytes[5], at + 5, 1);
            for (index, bytes) in bytes.as_chunks::<4>().0.iter().enumerate() {
                let word = u32::from_le_bytes(*bytes);
                self.field(
                    child,
                    format!("word_{:03X}", index * 4),
                    format!("{word:#010X} · f32 {}", f32::from_bits(word)),
                    at + index * 4,
                    4,
                );
            }
        }
        for cell in &file.cells {
            let at = base + cell.offset;
            let Some(child) = self.child(
                node,
                format!("空间网格 {}", cell.index),
                Kind::Block,
                buffer,
                at..at + 8,
            ) else {
                break;
            };
            self.field(child, "区域引用数", cell.count, at, 4);
            self.field(child, "引用列表相对偏移", cell.relative_offset, at + 4, 4);
            self.field(
                child,
                "区域记录相对偏移列表",
                hex(cell.references),
                base + cell.list_offset,
                cell.references.len(),
            );
        }
        if !file.trailing.is_empty() {
            self.field(
                node,
                "尾部原始字节",
                hex(file.trailing),
                base + file.source.len() - file.trailing.len(),
                file.trailing.len(),
            );
        }
    }
}
