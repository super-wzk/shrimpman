//! DAT 166 animation parameters, consumed by 10BBA7A0 and 10BBE150.
//! Repeat counts are signed; negative values are not decremented by the native
//! loops. Durations count native update steps, not an assumed wall-clock unit.

use super::{record, table};
use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RotationAnimation {
    pub start_degrees: i16,
    pub end_degrees: i16,
    pub repetitions: i16,
    pub duration: u16,
    pub state_rate_multiplier_bits: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScaleAnimation {
    pub start_bits: u32,
    pub end_bits: u32,
    /// State-dependent speed multiplier for Z; X/Y use has not been observed.
    pub parameter_08_bits: u32,
    pub repetitions: i16,
    pub duration: u16,
    /// Z selects stateful interpolation; X/Y use has not been observed.
    pub mode: u8,
    pub ping_pong: u8,
    pub unknown_12: [u8; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorAnimation {
    pub start_rgb: [u8; 3],
    pub end_rgb: [u8; 3],
    pub ping_pong: u16,
    pub repetitions: i16,
    pub duration: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpacityAnimation {
    /// Native storage is u16, even though the evaluated value is cast to u8.
    pub start: u16,
    pub end: u16,
    pub repetitions: i16,
    pub duration: u16,
    pub ping_pong: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UvAnimation {
    pub u_period: i16,
    pub v_period: i16,
    pub cycle_steps: u16,
    pub repetitions: i16,
    pub state_step_bits: u32,
    /// Nonzero adds the runtime step increment; 4 also uses 10BBDE80 mapping.
    pub mode: u8,
}

/// DAT entry 166. Every byte has an explicit field or an unknown range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelEffectDefinition {
    pub translation_delta_bits: [u32; 3],
    /// Selector for the native equipment-state predicate in 10BBCBD0.
    pub activation_condition: u8,
    pub draw_group: u8,
    pub group_entry: u8,
    pub node_index: u8,
    pub start_delay: u16,
    pub unknown_12: [u8; 2],
    pub rotation_state_flags: [u8; 3],
    pub unknown_17: u8,
    pub rotation: [RotationAnimation; 3],
    pub scale: [ScaleAnimation; 3],
    pub render_flags: u8,
    pub unknown_79: [u8; 3],
    pub color: ColorAnimation,
    pub opacity: OpacityAnimation,
    /// Value passed to native render-state key 0x60, without guessing an enum.
    pub render_state_60: u8,
    pub unknown_92: [u8; 2],
    pub uv: UvAnimation,
    pub unknown_a1: [u8; 7],
    pub weapon_visibility: u8,
    pub visibility_mode: u8,
    pub visibility_flags: u8,
    pub unknown_ab: u8,
    pub condition_transition: u8,
    /// Used for terminal initialization and the additional transform pass.
    pub terminal_transform: u8,
    pub unknown_ae: [u8; 6],
}

impl ModelEffectDefinition {
    pub const DAT_INDEX: usize = 166;
    pub const SIZE: usize = 180;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let b = record::<180>(bytes)?;
        let word = |at| u16::from_le_bytes([b[at], b[at + 1]]);
        let bits = |at| u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
        Ok(Self {
            translation_delta_bits: std::array::from_fn(|axis| bits(axis * 4)),
            activation_condition: b[12],
            draw_group: b[13],
            group_entry: b[14],
            node_index: b[15],
            start_delay: word(16),
            unknown_12: b[18..20].try_into().unwrap(),
            rotation_state_flags: b[20..23].try_into().unwrap(),
            unknown_17: b[23],
            rotation: std::array::from_fn(|axis| {
                let at = 24 + axis * 12;
                RotationAnimation {
                    start_degrees: word(at) as i16,
                    end_degrees: word(at + 2) as i16,
                    repetitions: word(at + 4) as i16,
                    duration: word(at + 6),
                    state_rate_multiplier_bits: bits(at + 8),
                }
            }),
            scale: std::array::from_fn(|axis| {
                let at = 60 + axis * 20;
                ScaleAnimation {
                    start_bits: bits(at),
                    end_bits: bits(at + 4),
                    parameter_08_bits: bits(at + 8),
                    repetitions: word(at + 12) as i16,
                    duration: word(at + 14),
                    mode: b[at + 16],
                    ping_pong: b[at + 17],
                    unknown_12: b[at + 18..at + 20].try_into().unwrap(),
                }
            }),
            render_flags: b[120],
            unknown_79: b[121..124].try_into().unwrap(),
            color: ColorAnimation {
                start_rgb: b[124..127].try_into().unwrap(),
                end_rgb: b[127..130].try_into().unwrap(),
                ping_pong: word(130),
                repetitions: word(132) as i16,
                duration: word(134),
            },
            opacity: OpacityAnimation {
                start: word(136),
                end: word(138),
                repetitions: word(140) as i16,
                duration: word(142),
                ping_pong: b[144],
            },
            render_state_60: b[145],
            unknown_92: b[146..148].try_into().unwrap(),
            uv: UvAnimation {
                u_period: word(148) as i16,
                v_period: word(150) as i16,
                cycle_steps: word(152),
                repetitions: word(154) as i16,
                state_step_bits: bits(156),
                mode: b[160],
            },
            unknown_a1: b[161..168].try_into().unwrap(),
            weapon_visibility: b[168],
            visibility_mode: b[169],
            visibility_flags: b[170],
            unknown_ab: b[171],
            condition_transition: b[172],
            terminal_transform: b[173],
            unknown_ae: b[174..180].try_into().unwrap(),
        })
    }

    pub fn parse_table(bytes: &[u8]) -> Result<Vec<Self>> {
        table::<180, _>(bytes, Self::parse)
    }

    pub fn translation_delta(&self) -> [f32; 3] {
        self.translation_delta_bits.map(f32::from_bits)
    }

    pub fn set_translation_delta(&mut self, translation: [f32; 3]) {
        self.translation_delta_bits = translation.map(f32::to_bits);
    }

    pub fn to_bytes(&self) -> [u8; 180] {
        let mut b = [0; 180];
        fn word(b: &mut [u8], at: usize, value: u16) {
            b[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        fn bits(b: &mut [u8], at: usize, value: u32) {
            b[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (axis, &value) in self.translation_delta_bits.iter().enumerate() {
            bits(&mut b, axis * 4, value);
        }
        b[12] = self.activation_condition;
        b[13] = self.draw_group;
        b[14] = self.group_entry;
        b[15] = self.node_index;
        word(&mut b, 16, self.start_delay);
        b[18..20].copy_from_slice(&self.unknown_12);
        b[20..23].copy_from_slice(&self.rotation_state_flags);
        b[23] = self.unknown_17;
        for (axis, channel) in self.rotation.iter().enumerate() {
            let at = 24 + axis * 12;
            word(&mut b, at, channel.start_degrees as u16);
            word(&mut b, at + 2, channel.end_degrees as u16);
            word(&mut b, at + 4, channel.repetitions as u16);
            word(&mut b, at + 6, channel.duration);
            bits(&mut b, at + 8, channel.state_rate_multiplier_bits);
        }
        for (axis, channel) in self.scale.iter().enumerate() {
            let at = 60 + axis * 20;
            bits(&mut b, at, channel.start_bits);
            bits(&mut b, at + 4, channel.end_bits);
            bits(&mut b, at + 8, channel.parameter_08_bits);
            word(&mut b, at + 12, channel.repetitions as u16);
            word(&mut b, at + 14, channel.duration);
            b[at + 16] = channel.mode;
            b[at + 17] = channel.ping_pong;
            b[at + 18..at + 20].copy_from_slice(&channel.unknown_12);
        }
        b[120] = self.render_flags;
        b[121..124].copy_from_slice(&self.unknown_79);
        b[124..127].copy_from_slice(&self.color.start_rgb);
        b[127..130].copy_from_slice(&self.color.end_rgb);
        word(&mut b, 130, self.color.ping_pong);
        word(&mut b, 132, self.color.repetitions as u16);
        word(&mut b, 134, self.color.duration);
        word(&mut b, 136, self.opacity.start);
        word(&mut b, 138, self.opacity.end);
        word(&mut b, 140, self.opacity.repetitions as u16);
        word(&mut b, 142, self.opacity.duration);
        b[144] = self.opacity.ping_pong;
        b[145] = self.render_state_60;
        b[146..148].copy_from_slice(&self.unknown_92);
        word(&mut b, 148, self.uv.u_period as u16);
        word(&mut b, 150, self.uv.v_period as u16);
        word(&mut b, 152, self.uv.cycle_steps);
        word(&mut b, 154, self.uv.repetitions as u16);
        bits(&mut b, 156, self.uv.state_step_bits);
        b[160] = self.uv.mode;
        b[161..168].copy_from_slice(&self.unknown_a1);
        b[168] = self.weapon_visibility;
        b[169] = self.visibility_mode;
        b[170] = self.visibility_flags;
        b[171] = self.unknown_ab;
        b[172] = self.condition_transition;
        b[173] = self.terminal_transform;
        b[174..180].copy_from_slice(&self.unknown_ae);
        b
    }
}
