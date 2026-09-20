//! Bounds-checked codec for the verified monster-selection instruction stream.
//!
//! These are logical linear instruction boundaries, not the cursor movement
//! of the native skip routine `10860730`. Compound selector-zero headers are
//! followed by a separate first branch marker; native handlers consume both
//! on entry. Keeping them separate preserves operands and block structure.
//! The opcode catalog records handler semantics.
//!
//! Every one of the 256 opcode bytes is covered. 137 of them are dispatched
//! by the interpreter switch at `10869761` and consume a fixed or
//! selector-dependent width. The remaining 119 bytes (see [`STOP_BYTES`]) fall
//! into the switch default at `1086A0BA`, which halts the script state, sets
//! the restart request and rewinds the cursor to `main[0]` before leaving the
//! dispatch loop.

use crate::ai::{Error, Result};

/// One instruction as it appears in a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instruction {
    /// Byte offset from the beginning of the script.
    pub offset: usize,
    /// The native opcode (the first byte of [`bytes`](Self::bytes)).
    pub opcode: u8,
    /// The complete instruction, including its opcode and operands.
    pub bytes: Vec<u8>,
}

const FIXED_ONE: &[u8] = &[
    0x04, 0x10, 0x11, 0x12, 0x13, 0x18, 0x19, 0x1e, 0x25, 0x2d, 0x30, 0x31, 0x4b, 0x4c, 0x4d, 0x52,
    0x53, 0x58, 0x5f, 0x61, 0x65, 0x68, 0x7b, 0x7e, 0x84, 0x85, 0x86, 0x92, 0x93,
];

const FIXED_TWO: &[u8] = &[
    0x01, 0x02, 0x03, 0x07, 0x08, 0x09, 0x0a, 0x0d, 0x16, 0x1f, 0x26, 0x28, 0x29, 0x2a, 0x2e, 0x2f,
    0x35, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x40, 0x41, 0x44, 0x45, 0x47, 0x48, 0x49, 0x4a, 0x4e,
    0x4f, 0x50, 0x51, 0x54, 0x55, 0x59, 0x5b, 0x5c, 0x5d, 0x60, 0x62, 0x66, 0x67, 0x72, 0x77, 0x7c,
    0x7f, 0x81, 0x90, 0x91, 0x99, 0xff,
];

/// Opcodes that hit the interpreter's switch default at `1086A0BA`.
///
/// The default case is not an error: when `+2739` (command mode) and `+2659`
/// (script state) are both zero the handler writes `+2659 = 2`, then always
/// sets the restart request `+2622 = 1`, rewinds the cursor with `10860430`
/// and leaves the dispatch loop. The byte itself is never skipped by a
/// handler, so [`instruction_len`] reports the single byte it occupies while
/// the interpreter treats it as a terminal reset.
pub const STOP_BYTES: &[u8] = &[
    0x00, 0x43, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e,
    0x8f, 0x95, 0x96, 0x97, 0x98, 0x9d, 0x9e, 0x9f, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf, 0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7,
    0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
    0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7,
    0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf, 0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7,
    0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef, 0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7,
    0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe,
];

/// Whether `opcode` reaches the interpreter's switch default.
pub fn is_stop(opcode: u8) -> bool {
    STOP_BYTES.contains(&opcode)
}

fn need(bytes: &[u8], length: usize) -> Result<()> {
    if bytes.len() >= length {
        return Ok(());
    }
    let opcode = bytes.first().copied().unwrap_or_default();
    Err(Error::new(format!(
        "truncated opcode {opcode:#04x}: need {length} bytes, have {}",
        bytes.len()
    )))
}

fn subtype(bytes: &[u8], _opcode: u8) -> Result<u8> {
    need(bytes, 2)?;
    Ok(bytes[1])
}

/// Return one logical instruction's width, including its own operands only.
///
/// For list/choice families, selector zero owns the count (and, for 79, the
/// callback argument), not the following branch marker or its operand. Native
/// skip widths instead cover header + first marker (e.g. 70: 6, 15: 7, 80: 6,
/// 83: 5). 94 is especially different: its skip width is 5, landing on the
/// first case operand, while handler 108682B0 consumes that operand as well.
/// Neither skip widths nor execution cursor deltas are linear token widths.
/// Unknown selector forms retain the legacy codec widths but are rejected by
/// structure validation. Default opcodes occupy one byte.
pub fn instruction_len(bytes: &[u8]) -> Result<usize> {
    need(bytes, 1)?;
    let opcode = bytes[0];
    if is_stop(opcode) {
        return Ok(1);
    }
    let length = if FIXED_ONE.contains(&opcode) {
        1
    } else if FIXED_TWO.contains(&opcode) {
        2
    } else {
        match opcode {
            0x05 | 0x3f => 4,
            0x06 => {
                if subtype(bytes, opcode)? == 3 {
                    5
                } else {
                    4
                }
            }
            // Subtype 0 carries a three-byte form; 1 and 2 are two-byte
            // markers.  0x14 is the verified angle-condition family.
            0x0b | 0x14 | 0x1b | 0x22 | 0x32 | 0x36 | 0x42 | 0x5a | 0x5e | 0x63 | 0x64 | 0x69
            | 0x71 | 0x74 => match subtype(bytes, opcode)? {
                0 => 3,
                1 | 2 => 2,
                _ => 2,
            },
            // These dispatch directly to the native three-byte skip handler;
            // their second byte is an operand, not a subtype selector.
            0x0c | 0x1a | 0x82 => 3,
            // Subtype 0 carries one additional two-byte field here; 1 and 2
            // are short forms.
            0x0e | 0x2b | 0x34 | 0x3d | 0x46 | 0x56 | 0x78 | 0x9a | 0x9b | 0x9c => {
                match subtype(bytes, opcode)? {
                    0 => 4,
                    1 | 2 => 2,
                    _ => 2,
                }
            }
            0x0f => 6,
            0x15 => match subtype(bytes, opcode)? {
                // 10861BB0: count, then a separate 15/1 + be_u16 case.
                0 => 3,
                1 => 4,
                2 | 3 => 2,
                _ => 2,
            },
            0x17 => 5,
            0x1c | 0x1d | 0x20 | 0x23 | 0x27 | 0x2c | 0x33 | 0x70 | 0x73 | 0x75 | 0x76 | 0x7a
            | 0x7d => match subtype(bytes, opcode)? {
                // Count, then a separate opcode/1 + u8 case.
                0 => 3,
                1 => 3,
                _ => 2,
            },
            // The native function increments to the selector and returns that
            // pointer for unknown values, so this is one byte (opcode only).
            0x21 => match subtype(bytes, opcode)? {
                0..=2 => 2,
                _ => 1,
            },
            0x24 => match subtype(bytes, opcode)? {
                0 => 3,
                1 => 2,
                _ => 1,
            },
            0x3e | 0x57 => match subtype(bytes, opcode)? {
                // 10863A40/10865910: count, then opcode/1 + be_u16.
                0 => 3,
                1 => 4,
                2 | 3 => 2,
                _ => 1,
            },
            0x79 => match subtype(bytes, opcode)? {
                // 10862D20: count and callback argument, then 79/1 + u8.
                0 => 4,
                1 => 3,
                _ => 2,
            },
            0x80 => match subtype(bytes, opcode)? {
                // 10866E50 scans from the end of this count header.
                0 => 3,
                1..=0x20 => 3,
                0xff => 2,
                _ => 1,
            },
            0x83 => match subtype(bytes, opcode)? {
                // 10867460: count, then a separate two-byte branch marker.
                0 => 3,
                1..=5 | 0xff => 2,
                _ => 1,
            },
            0x94 => match subtype(bytes, opcode)? {
                0 => 3,
                1 => 3,
                _ => 2,
            },
            value => {
                return Err(Error::new(format!(
                    "opcode {value:#04x} is neither dispatched nor a stop byte"
                )));
            }
        }
    };
    need(bytes, length)?;
    Ok(length)
}

/// Decode every instruction in a script, checking each operand boundary.
pub fn decode(mut bytes: &[u8]) -> Result<Vec<Instruction>> {
    let mut instructions = Vec::new();
    let mut offset = 0usize;
    while !bytes.is_empty() {
        let length = instruction_len(bytes)
            .map_err(|error| Error::new(format!("script offset {offset:#x}: {error}")))?;
        instructions.push(Instruction {
            offset,
            opcode: bytes[0],
            bytes: bytes[..length].to_vec(),
        });
        bytes = &bytes[length..];
        offset = offset
            .checked_add(length)
            .ok_or_else(|| Error::new("script offset overflows usize"))?;
    }
    Ok(instructions)
}

/// Check the native marker scans before publishing a standalone allocation.
/// Width-valid bytes can still hang 10860A10: it has no end pointer and does
/// not advance on a default opcode while looking for a missing closing marker.
pub fn validate_structure(bytes: &[u8]) -> Result<()> {
    let mut structure = ScriptStructure::default();
    for instruction in decode(bytes)? {
        structure.push(&instruction.bytes).map_err(|error| {
            Error::new(format!("script offset {:#x}: {error}", instruction.offset))
        })?;
    }
    if let Some(&(opcode, close)) = structure.blocks.last() {
        return Err(Error::new(format!(
            "未闭合的 AI 条件块 {opcode:#04x}：缺少 native({opcode:#04x}, {close:#04x});，拒绝安装以避免原生扫描卡死；请重新反编译或补全脚本"
        )));
    }
    Ok(())
}

/// Shared by bounded extraction and installation validation. These families
/// come from ALL callers of 10860A10 (including fixed-width instructions).
#[derive(Default)]
pub(crate) struct ScriptStructure {
    blocks: Vec<(u8, u8)>,
    pending_first_branch: Option<u8>,
}

impl ScriptStructure {
    pub(crate) fn is_closed(&self) -> bool {
        self.blocks.is_empty()
    }

    /// How many native blocks are still open. Used to prove that nothing but
    /// closing markers follows a handler's early exit.
    pub(crate) fn depth(&self) -> usize {
        self.blocks.len()
    }

    pub(crate) fn push(&mut self, instruction: &[u8]) -> Result<()> {
        let opcode = instruction[0];
        let selector = instruction.get(1).copied();
        if let Some(expected) = self.pending_first_branch.take()
            && (opcode != expected || selector != Some(1))
        {
            return Err(Error::new(format!(
                "compound header {expected:#04x} requires its first branch marker"
            )));
        }
        // 24 and 62 have execution/skip-width disagreements; FF's unknown
        // selectors redispatch rather than consuming a two-byte instruction.
        if matches!(opcode, 0x24 | 0x62)
            || (opcode == 0xff && !matches!(selector, Some(0..=6 | 0xf5..=0xff)))
        {
            return Err(Error::new(format!(
                "opcode {opcode:#04x} 的执行边界尚未支持，不能独立安装"
            )));
        }
        if is_stop(opcode) && !self.is_closed() {
            return Err(Error::new(
                "条件块内的 default 终止字节会使原生标记扫描无法前进",
            ));
        }
        let close = match opcode {
            0x01 | 0x02 | 0x03 | 0x08 | 0x09 | 0x0b | 0x0e | 0x14 | 0x1b | 0x1f | 0x21 | 0x22
            | 0x28 | 0x29 | 0x2a | 0x2b | 0x2f | 0x32 | 0x34 | 0x35 | 0x36 | 0x37 | 0x38 | 0x39
            | 0x3a | 0x3b | 0x3c | 0x3d | 0x42 | 0x44 | 0x45 | 0x46 | 0x47 | 0x4a | 0x51 | 0x54
            | 0x55 | 0x56 | 0x59 | 0x5a | 0x5c | 0x5d | 0x5e | 0x60 | 0x63 | 0x64 | 0x66 | 0x67
            | 0x69 | 0x71 | 0x72 | 0x74 | 0x77 | 0x78 | 0x7c | 0x7f | 0x9a | 0x9b | 0x9c => 2,
            0x15 | 0x1c | 0x1d | 0x20 | 0x23 | 0x27 | 0x2c | 0x33 | 0x3e | 0x57 | 0x70 | 0x73
            | 0x75 | 0x76 | 0x79 | 0x7a | 0x7d | 0x94 => 3,
            0x80 | 0x83 => 0xff,
            _ => return Ok(()),
        };
        let selector = selector.ok_or_else(|| Error::new("ambiguous selector width"))?;
        let valid = match opcode {
            0x80 => selector <= 0x1f || selector == 0xff,
            0x83 => selector <= 5 || selector == 0xff,
            _ => selector <= close,
        };
        if !valid {
            return Err(Error::new("unconfirmed native selector boundary"));
        }
        if selector == 0 {
            self.blocks.push((opcode, close));
            if close == 3 || close == 0xff {
                self.pending_first_branch = Some(opcode);
            }
        } else if self.blocks.last() != Some(&(opcode, close)) {
            return Err(Error::new(format!(
                "AI 标记 {opcode:#04x}/{selector:#04x} 没有匹配的条件块"
            )));
        } else if selector == close {
            self.blocks.pop();
        }
        Ok(())
    }
}

/// Encode the verified action-selection instruction (`0x05`).
///
/// The fourth byte is passed through to the native handler; its semantics are
/// not assumed here and must be retained per monster/action catalog.
pub fn encode_action(group: u8, id: u8, parameter: u8) -> [u8; 4] {
    [0x05, group, id, parameter]
}

/// Encode a jump to one of the root table's main-script entries (`0x07`).
pub fn encode_main_jump(index: u8) -> [u8; 2] {
    [0x07, index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_native_width_families() {
        let bytes = [
            0x05, 0x01, 0x02, 0xaa, // action
            0x14, 0, 45, // angle condition
            0x14, 1, // angle marker
            0x14, 2, // angle marker
            0x15, 0, 1, // list count header
            0x15, 1, 4, 5, // first case (be_u16 operand)
            0x15, 2, // native short subtype
            0x15, 3, // native short subtype
            0x80, 1, 9, // short subtype
            0x80, 0xff, // native short marker
            0x0c, 0xff, 9, // fixed-width operand form
            0x1a, 3, 8, // fixed-width operand form
            0x82, 0, 7, // fixed-width operand form
            0x07, 3, // main jump
        ];
        let decoded = decode(&bytes).unwrap();
        assert_eq!(
            decoded
                .iter()
                .map(|item| item.bytes.len())
                .collect::<Vec<_>>(),
            [4, 3, 2, 2, 3, 4, 2, 2, 3, 2, 3, 3, 3, 2]
        );
        assert_eq!(decoded[4].offset, 11);
    }

    #[test]
    fn rejects_truncation_at_each_variable_boundary() {
        for full in [
            vec![0x05, 0, 0, 0],
            vec![0x14, 0, 0],
            vec![0x15, 0, 1],
            vec![0x15, 1, 0, 0],
            vec![0x79, 0, 1, 4],
            vec![0x94, 0, 2],
            vec![0x80, 1, 0],
            vec![0x83, 0, 1],
        ] {
            for length in 0..full.len() {
                assert!(
                    instruction_len(&full[..length]).is_err(),
                    "opcode {:02x}, {length}",
                    full[0]
                );
            }
            assert_eq!(instruction_len(&full).unwrap(), full.len());
        }
    }

    #[test]
    fn preserves_legacy_unknown_selector_widths() {
        assert_eq!(instruction_len(&[0x14, 3]).unwrap(), 2);
        assert_eq!(instruction_len(&[0x0e, 3]).unwrap(), 2);
        assert_eq!(instruction_len(&[0x15, 4]).unwrap(), 2);
        assert_eq!(instruction_len(&[0x1c, 3]).unwrap(), 2);
        assert_eq!(instruction_len(&[0x21, 3]).unwrap(), 1);
        assert_eq!(instruction_len(&[0x24, 3]).unwrap(), 1);
        assert_eq!(instruction_len(&[0x3e, 4]).unwrap(), 1);
        assert_eq!(instruction_len(&[0x57, 4]).unwrap(), 1);
        assert_eq!(instruction_len(&[0x79, 2]).unwrap(), 2);
        assert_eq!(instruction_len(&[0x80, 0x21]).unwrap(), 1);
        assert_eq!(instruction_len(&[0x83, 6]).unwrap(), 1);
        assert_eq!(instruction_len(&[0x94, 2]).unwrap(), 2);
    }

    #[test]
    fn covers_every_opcode_byte() {
        assert_eq!(instruction_len(&[0x00]).unwrap(), 1);
        assert_eq!(instruction_len(&[0x43]).unwrap(), 1);
        assert_eq!(instruction_len(&[0xfe]).unwrap(), 1);
        assert!(is_stop(0x43));
        assert!(!is_stop(0x05));
        assert_eq!(STOP_BYTES.len(), 119);
        let dispatched = (0..=0xffu8).filter(|opcode| !is_stop(*opcode)).count();
        assert_eq!(dispatched, 137);
        for opcode in 0..=0xffu8 {
            let width = instruction_len(&[opcode, 0, 0, 0, 0, 0, 0])
                .unwrap_or_else(|error| panic!("opcode {opcode:#04x}: {error}"));
            assert!((1..=7).contains(&width), "opcode {opcode:#04x}");
        }
        for stop in STOP_BYTES {
            assert!(
                !FIXED_ONE.contains(stop),
                "stop byte {stop:#04x} is a fixed one-byte opcode"
            );
            assert!(
                !FIXED_TWO.contains(stop),
                "stop byte {stop:#04x} is a fixed two-byte opcode"
            );
        }
    }

    #[test]
    fn compound_headers_leave_first_markers_and_zero_operands_visible() {
        for opcode in [
            0x15, 0x1c, 0x1d, 0x20, 0x23, 0x27, 0x2c, 0x33, 0x3e, 0x57, 0x70, 0x73, 0x75, 0x76,
            0x79, 0x7a, 0x7d, 0x80, 0x83, 0x94,
        ] {
            let mut header = vec![opcode, 0, 1];
            if opcode == 0x79 {
                header.push(4);
            }
            let mut marker = vec![opcode, 1];
            if opcode != 0x83 {
                marker.push(0);
            }
            if matches!(opcode, 0x15 | 0x3e | 0x57) {
                marker.push(0);
            }
            let close = if matches!(opcode, 0x80 | 0x83) {
                0xff
            } else {
                3
            };
            let mut bytes = header.clone();
            bytes.extend_from_slice(&marker);
            // An early reset must not hide the closing marker.
            bytes.extend_from_slice(&[0xff, 0, opcode, close, 0xff, 0]);
            let decoded = decode(&bytes).unwrap();
            assert_eq!(decoded[0].bytes, header);
            assert_eq!(decoded[1].bytes, marker);
            assert_eq!(decoded[1].offset, header.len());
            validate_structure(&bytes).unwrap();
            assert!(validate_structure(&bytes[..header.len() + marker.len() + 2]).is_err());
            // Losing a first marker must not become a valid, shorter block.
            let mut missing = header.clone();
            missing.extend_from_slice(&[opcode, close]);
            assert!(validate_structure(&missing).is_err());
            // A genuine default opcode inside a block is still unsafe.
            bytes[header.len() + marker.len()] = 0;
            assert!(validate_structure(&bytes).is_err());
        }
    }

    #[test]
    fn action_and_jump_encoding_match_native_prefixes() {
        assert_eq!(encode_action(3, 6, 0), [5, 3, 6, 0]);
        assert_eq!(encode_main_jump(7), [7, 7]);
    }
}
