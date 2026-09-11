//! DAT 161, initialized by 10BB0830 and evaluated by 10BB2AB0.
//! Unlike DAT 166, rotation has no state-rate field and scaling is uniform.

use super::{ColorAnimation, OpacityAnimation, record, table};
use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentSequence {
    /// Animated ID passed to the resource lookup 10BBA050.
    pub start_id: i16,
    pub end_id: i16,
    pub repetitions: i16,
    pub interval: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentRotation {
    pub start_degrees: i16,
    pub end_degrees: i16,
    pub repetitions: i16,
    pub duration: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentScale {
    pub start_bits: u32,
    pub end_bits: u32,
    pub ping_pong: u16,
    pub repetitions: i16,
    pub duration: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentUvAnimation {
    pub u_period: i16,
    pub v_period: i16,
    pub cycle_steps: u16,
    pub repetitions: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentDefinition {
    pub local_position_bits: [u32; 3],
    pub activation_condition: u8,
    pub node_index: u8,
    pub attachment_mode: u8,
    pub unknown_0f: u8,
    pub resource_id: u16,
    pub start_delay: u16,
    pub sequence: AttachmentSequence,
    pub rotation: [AttachmentRotation; 3],
    pub scale: AttachmentScale,
    pub unknown_42: [u8; 4],
    pub render_flags: u8,
    /// Selects native orientation-matrix branches; not assumed to be XYZ bits.
    pub orientation_flags: [u8; 3],
    pub color: ColorAnimation,
    pub opacity: OpacityAnimation,
    pub render_state_60: u8,
    pub unknown_60: [u8; 2],
    pub uv: AttachmentUvAnimation,
    pub unknown_6a: [u8; 2],
    pub view_offset_bits: u32,
    /// 0: no trail, 1: 10BB4E60, 2: 10BB5A90; other values retained.
    pub trail_mode: u8,
    pub trail_rgb: [u8; 3],
    pub weapon_visibility: u8,
    pub visibility_mode: u8,
    pub visibility_flags: u8,
    pub unknown_77: [u8; 2],
    pub terminal_initialization: u8,
    pub unknown_7a: [u8; 6],
}

impl AttachmentDefinition {
    pub const DAT_INDEX: usize = 161;
    pub const SIZE: usize = 128;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let b = record::<128>(bytes)?;
        let word = |at| u16::from_le_bytes([b[at], b[at + 1]]);
        let bits = |at| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        Ok(Self {
            local_position_bits: std::array::from_fn(|axis| bits(axis * 4)),
            activation_condition: b[12],
            node_index: b[13],
            attachment_mode: b[14],
            unknown_0f: b[15],
            resource_id: word(16),
            start_delay: word(18),
            sequence: AttachmentSequence {
                start_id: word(20) as i16,
                end_id: word(22) as i16,
                repetitions: word(24) as i16,
                interval: word(26) as i16,
            },
            rotation: std::array::from_fn(|axis| {
                let at = 28 + axis * 8;
                AttachmentRotation {
                    start_degrees: word(at) as i16,
                    end_degrees: word(at + 2) as i16,
                    repetitions: word(at + 4) as i16,
                    duration: word(at + 6),
                }
            }),
            scale: AttachmentScale {
                start_bits: bits(52),
                end_bits: bits(56),
                ping_pong: word(60),
                repetitions: word(62) as i16,
                duration: word(64),
            },
            unknown_42: b[66..70].try_into().unwrap(),
            render_flags: b[70],
            orientation_flags: b[71..74].try_into().unwrap(),
            color: ColorAnimation {
                start_rgb: b[74..77].try_into().unwrap(),
                end_rgb: b[77..80].try_into().unwrap(),
                ping_pong: word(80),
                repetitions: word(82) as i16,
                duration: word(84),
            },
            opacity: OpacityAnimation {
                start: word(86),
                end: word(88),
                repetitions: word(90) as i16,
                duration: word(92),
                ping_pong: b[94],
            },
            render_state_60: b[95],
            unknown_60: b[96..98].try_into().unwrap(),
            uv: AttachmentUvAnimation {
                u_period: word(98) as i16,
                v_period: word(100) as i16,
                cycle_steps: word(102),
                repetitions: word(104) as i16,
            },
            unknown_6a: b[106..108].try_into().unwrap(),
            view_offset_bits: bits(108),
            trail_mode: b[112],
            trail_rgb: b[113..116].try_into().unwrap(),
            weapon_visibility: b[116],
            visibility_mode: b[117],
            visibility_flags: b[118],
            unknown_77: b[119..121].try_into().unwrap(),
            terminal_initialization: b[121],
            unknown_7a: b[122..128].try_into().unwrap(),
        })
    }

    pub fn parse_table(bytes: &[u8]) -> Result<Vec<Self>> {
        table::<128, _>(bytes, Self::parse)
    }
    pub fn local_position(&self) -> [f32; 3] {
        self.local_position_bits.map(f32::from_bits)
    }
    pub fn set_local_position(&mut self, position: [f32; 3]) {
        self.local_position_bits = position.map(f32::to_bits);
    }

    pub fn to_bytes(&self) -> [u8; 128] {
        let mut b = [0; 128];
        fn word(b: &mut [u8], at: usize, value: u16) {
            b[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn bits(b: &mut [u8], at: usize, value: u32) {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (axis, &value) in self.local_position_bits.iter().enumerate() {
            bits(&mut b, axis * 4, value);
        }
        b[12] = self.activation_condition;
        b[13] = self.node_index;
        b[14] = self.attachment_mode;
        b[15] = self.unknown_0f;
        word(&mut b, 16, self.resource_id);
        word(&mut b, 18, self.start_delay);
        word(&mut b, 20, self.sequence.start_id as u16);
        word(&mut b, 22, self.sequence.end_id as u16);
        word(&mut b, 24, self.sequence.repetitions as u16);
        word(&mut b, 26, self.sequence.interval as u16);
        for (axis, channel) in self.rotation.iter().enumerate() {
            let at = 28 + axis * 8;
            word(&mut b, at, channel.start_degrees as u16);
            word(&mut b, at + 2, channel.end_degrees as u16);
            word(&mut b, at + 4, channel.repetitions as u16);
            word(&mut b, at + 6, channel.duration);
        }
        bits(&mut b, 52, self.scale.start_bits);
        bits(&mut b, 56, self.scale.end_bits);
        word(&mut b, 60, self.scale.ping_pong);
        word(&mut b, 62, self.scale.repetitions as u16);
        word(&mut b, 64, self.scale.duration);
        b[66..70].copy_from_slice(&self.unknown_42);
        b[70] = self.render_flags;
        b[71..74].copy_from_slice(&self.orientation_flags);
        b[74..77].copy_from_slice(&self.color.start_rgb);
        b[77..80].copy_from_slice(&self.color.end_rgb);
        word(&mut b, 80, self.color.ping_pong);
        word(&mut b, 82, self.color.repetitions as u16);
        word(&mut b, 84, self.color.duration);
        word(&mut b, 86, self.opacity.start);
        word(&mut b, 88, self.opacity.end);
        word(&mut b, 90, self.opacity.repetitions as u16);
        word(&mut b, 92, self.opacity.duration);
        b[94] = self.opacity.ping_pong;
        b[95] = self.render_state_60;
        b[96..98].copy_from_slice(&self.unknown_60);
        word(&mut b, 98, self.uv.u_period as u16);
        word(&mut b, 100, self.uv.v_period as u16);
        word(&mut b, 102, self.uv.cycle_steps);
        word(&mut b, 104, self.uv.repetitions as u16);
        b[106..108].copy_from_slice(&self.unknown_6a);
        bits(&mut b, 108, self.view_offset_bits);
        b[112] = self.trail_mode;
        b[113..116].copy_from_slice(&self.trail_rgb);
        b[116] = self.weapon_visibility;
        b[117] = self.visibility_mode;
        b[118] = self.visibility_flags;
        b[119..121].copy_from_slice(&self.unknown_77);
        b[121] = self.terminal_initialization;
        b[122..128].copy_from_slice(&self.unknown_7a);
        b
    }
}
