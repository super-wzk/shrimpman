use super::super::{Builder, Kind, hex};
use crate::field::FieldReference;
use mhf_monster::ai::{
    Error, Result, bytecode,
    decompile::{Memory, extract_script},
};

struct FileMemory<'a>(&'a [u8]);

impl Memory for FileMemory<'_> {
    fn bytes(&self, address: u32, length: usize) -> Result<Vec<u8>> {
        let start = address as usize;
        let end = start
            .checked_add(length)
            .ok_or_else(|| Error::new("EMD AI address overflow"))?;
        self.0
            .get(start..end)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| Error::new(format!("EMD AI read outside resource at {address:#010x}")))
    }
}

fn instructions(bytes: &[u8], offset: usize) -> Result<Vec<bytecode::Instruction>> {
    let address = u32::try_from(offset).map_err(|_| Error::new("EMD AI offset exceeds u32"))?;
    // EMD entries do not provide the DLL descriptor or call level, so no
    // same-level tail call is assumed from an 81/82 instruction alone.
    bytecode::decode(&extract_script(&FileMemory(bytes), address, None)?)
}

impl Builder {
    pub(super) fn emd_ai_script_fields(
        &mut self,
        node: usize,
        bytes: &[u8],
        base: usize,
        offset: usize,
    ) -> std::result::Result<(), String> {
        if offset >= bytes.len() {
            return Err("EMD AI 偏移超出资源".into());
        }
        let at = base.checked_add(offset).ok_or("EMD AI 地址溢出")?;
        let instructions = match instructions(bytes, offset) {
            Ok(instructions) => instructions,
            Err(error) => {
                let tail = &bytes[offset..];
                let preview = &tail[..tail.len().min(32)];
                self.field(node, "原始预览", hex(preview), at, preview.len());
                return Err(error.to_string());
            }
        };
        let size = instructions.last().map_or(0, |instruction| {
            instruction.offset + instruction.bytes.len()
        });
        let end = at.checked_add(size).ok_or("EMD AI 范围溢出")?;
        self.document.nodes[node].range = at..end;
        self.field(node, "指令数", instructions.len(), at, 0);
        self.field(node, "脚本字节数", size, at, 0);
        let buffer = self.document.nodes[node].buffer;
        for (index, instruction) in instructions.iter().enumerate() {
            let position = at + instruction.offset;
            let Some(child) = self.child(
                node,
                format!("指令 {index:03} · {:#04x}", instruction.opcode),
                Kind::EmdAiInstruction,
                buffer,
                position..position + instruction.bytes.len(),
            ) else {
                break;
            };
            self.read::<u8>(child, "opcode", position)?;
            for operand in 1..instruction.bytes.len() {
                self.read::<u8>(child, format!("operand_{operand:02}"), position + operand)?;
            }
            self.field(
                child,
                "raw_bytes",
                hex(&instruction.bytes),
                position,
                instruction.bytes.len(),
            );
            let target = match instruction.bytes.as_slice() {
                [0x16, index] => Some((9usize, *index)),
                [0x81, index] => Some((1, *index)),
                [0x82, group, index] => Some((15 + usize::from(*group), *index)),
                _ => None,
            };
            if let Some((table, index)) = target {
                let field = self.document.nodes[child].fields.len();
                self.field(child, "子脚本引用", "", position, 0);
                self.document.nodes[child].fields[field].reference =
                    Some(FieldReference::NativeScript {
                        table: table as u32,
                        index: u32::from(index),
                    });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::FieldType;
    use crate::inspect::inspect;

    fn script_bytes(script: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; 16];
        bytes.extend_from_slice(script);
        bytes
    }

    #[test]
    fn unknown_levels_keep_calls_until_a_closed_return() {
        for ending in [1, 2, 3] {
            let script = [0x81, 4, 0x82, 3, 7, 0x16, 2, 0xff, ending];
            let decoded = instructions(&script_bytes(&script), 16).unwrap();
            assert_eq!(decoded.len(), 4);
            assert_eq!(
                decoded
                    .into_iter()
                    .flat_map(|i| i.bytes)
                    .collect::<Vec<_>>(),
                script
            );
        }
    }

    #[test]
    fn nested_returns_retain_closing_markers_without_reading_past_the_end() {
        let script = [0x35, 0, 0xff, 1, 0x35, 2, 0xff, 2];
        let decoded = instructions(&script_bytes(&script), 16).unwrap();
        assert_eq!(decoded.last().unwrap().offset, 6);
        assert_eq!(decoded.last().unwrap().bytes, [0xff, 2]);
    }

    #[test]
    fn invalid_script_boundaries_are_reported() {
        for script in [
            &[0x05, 1][..],
            &[0x80, 2, 16],
            &[0xff, 7],
            &[0x35, 0, 0xff, 1],
        ] {
            assert!(
                instructions(&script_bytes(script), 16).is_err(),
                "{script:02x?}"
            );
        }
        assert!(instructions(&[0; 16], 16).is_err());
        assert!(instructions(&[0; 16], usize::MAX).is_err());
        assert!(instructions(&[0; 16], 0).is_err());
    }

    #[test]
    fn instruction_fields_bind_original_bytes_and_identify_external_calls() {
        let script = [0x81, 7, 0x82, 3, 4, 0x16, 9, 0xff, 2];
        let file = script_bytes(&script);
        let base = 7;
        let mut buffer = vec![0xaa; base];
        buffer.extend_from_slice(&file);
        let mut builder = Builder {
            document: inspect("script.bytes", buffer.into()),
            parents: vec![None],
            work: Vec::new(),
        };
        let at = base + 16;
        let node = builder
            .child(0, "AI", Kind::EmdAiScript(0), 0, at..at + 1)
            .unwrap();
        builder.emd_ai_script_fields(node, &file, base, 16).unwrap();
        assert_eq!(builder.document.nodes[node].range, at..at + script.len());
        let children = &builder.document.nodes[node].children;
        assert_eq!(children.len(), 4);
        for (index, (table, record)) in [(1, 7), (18, 4), (9, 9)].into_iter().enumerate() {
            let instruction = &builder.document.nodes[children[index]];
            assert!(
                instruction
                    .fields
                    .iter()
                    .any(|field| matches!(field.reference, Some(FieldReference::NativeScript { table: actual_table, index: actual_index }) if actual_table == table && actual_index == record))
            );
        }
        let first = &builder.document.nodes[children[0]];
        let operand = first
            .fields
            .iter()
            .find(|field| field.name == "operand_01")
            .unwrap();
        assert_eq!(operand.binding.range, at + 1..at + 2);
        let raw = first
            .fields
            .iter()
            .find(|field| field.name == "raw_bytes")
            .unwrap();
        assert_eq!(raw.binding.range, at..at + 2);
        assert_eq!(raw.binding.format, FieldType::Bytes);
        assert!(raw.writable);
    }

    #[test]
    fn malformed_entry_retains_a_bounded_preview_without_blocking_another_entry() {
        let file = script_bytes(&[0x80, 2, 16, 0xff, 1]);
        let mut builder = Builder {
            document: inspect("script.bytes", file.clone().into()),
            parents: vec![None],
            work: Vec::new(),
        };
        let bad = builder
            .child(0, "bad", Kind::EmdAiScript(0), 0, 16..17)
            .unwrap();
        let good = builder
            .child(0, "good", Kind::EmdAiScript(1), 0, 19..20)
            .unwrap();
        let error = builder.emd_ai_script_fields(bad, &file, 0, 16).unwrap_err();
        assert!(error.contains("0x80/0x02"), "{error}");
        assert!(builder.document.nodes[bad].children.is_empty());
        let preview = builder.document.nodes[bad]
            .fields
            .iter()
            .find(|field| field.binding.format == FieldType::Bytes)
            .unwrap();
        assert_eq!(preview.binding.range, 16..file.len());
        builder.emd_ai_script_fields(good, &file, 0, 19).unwrap();
        assert_eq!(builder.document.nodes[good].range, 19..21);
        assert_eq!(builder.document.nodes[good].children.len(), 1);
    }

    #[test]
    #[ignore = "requires local mhfemd.bin via MHF_EMD_PATH"]
    fn real_emd_scripts_preserve_independent_validation() {
        let source =
            std::fs::read(std::env::var_os("MHF_EMD_PATH").expect("set MHF_EMD_PATH")).unwrap();
        let opened = mhf_resource::container::open_layers(&source, 64 * 1024 * 1024, 8).unwrap();
        let bytes = opened.payload();
        let file = mhf_resource::emd::Emd::parse(bytes).unwrap();
        let table = file.root_table(9).unwrap().unwrap();
        assert_eq!(table.count, 505);
        let mut failures = Vec::new();
        let mut valid = 0;
        for index in 0..table.count {
            let (_, record) = table.record(index).unwrap();
            let offset = u32::from_le_bytes(record.try_into().unwrap()) as usize;
            match instructions(bytes, offset) {
                Ok(script) => {
                    assert!(!script.is_empty(), "root9[{index}]");
                    valid += 1;
                }
                Err(error) => failures.push((index, error.to_string())),
            }
        }
        for record in super::super::relations::script_links(&file).records {
            if record.reference.slot != 9 || record.links.is_empty() {
                continue;
            }
            let index = record.reference.record;
            assert!(
                failures.iter().all(|(failed, _)| *failed != index),
                "associated root9[{index}] failed: {failures:?}"
            );
        }
        eprintln!(
            "root9: {valid}/{} extracted; failures: {failures:?}",
            table.count
        );
        let known_invalid = failures.iter().find(|(index, _)| *index == 274).unwrap();
        assert!(known_invalid.1.contains("0x80/0x02"), "{failures:?}");
        assert_eq!(
            failures.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            [
                214, 215, 216, 217, 218, 219, 220, 221, 224, 256, 257, 258, 259, 260, 261, 270,
                271, 274, 289, 290, 291, 298, 299, 300, 301, 302, 303, 304, 305, 306, 481, 489,
                493, 497, 501,
            ],
            "verified samples must retain diagnostics for entries whose boundaries are not established"
        );
        let table = file.root_table(17).unwrap().unwrap();
        assert_eq!(table.count, 85);
        for index in 0..table.count {
            let (_, record) = table.record(index).unwrap();
            let offset = u32::from_le_bytes(record[4..8].try_into().unwrap()) as usize;
            let script = instructions(bytes, offset)
                .unwrap_or_else(|error| panic!("root17[{index}]: {error}"));
            assert_eq!(script.last().unwrap().bytes, [0xff, 1]);
        }
        eprintln!("root17: {}/{} extracted", table.count, table.count);
    }
}
