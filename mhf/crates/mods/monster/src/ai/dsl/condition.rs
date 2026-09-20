//! Semantic conditions and their verified native control-flow encodings.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Active,
    TargetAngleIn { min: Degrees, max: Degrees },
    Flashed,
    Enraged,
    TargetAvailable,
    CheckTrackedPlayers,
    ModeIs(Mode),
}

/// Finite, nonnegative degrees stored by bits to keep syntax trees equatable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Degrees(u64);

impl Degrees {
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..=360.0).contains(&value)).then_some(Self(value.to_bits()))
    }
    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
    pub(crate) fn native(self) -> u8 {
        (self.value() / 1.40625).round().min(255.0) as u8
    }
    pub(crate) fn from_native(value: u8) -> Self {
        Self((f64::from(value) * 1.40625).to_bits())
    }
}

/// Built-in mode names; Normal is a DSL name, Attack is native EM_MODE_ATTACK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Normal = 0,
    Attack = 1,
}

impl Mode {
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "Normal" => Some(Self::Normal),
            "Attack" => Some(Self::Attack),
            _ => None,
        }
    }

    pub(crate) fn from_native(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Normal),
            1 => Some(Self::Attack),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Normal => "Mode::Normal",
            Self::Attack => "Mode::Attack",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConditionMarker {
    Begin(Condition),
    Else,
    End,
    /// A known condition family with an operand/layout the DSL cannot express.
    /// Unlike a non-condition instruction, this prevents structured recovery.
    Unsupported,
}

impl ConditionMarker {
    /// Decode a complete instruction, never inventing missing condition operands.
    /// The request protocol's `1B 00 01` and the generic `79` callback blocks
    /// need their whole enclosing structure, so both stay unsupported here.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        use ConditionMarker::{Begin, Else, End, Unsupported};

        let (&opcode, operands) = bytes.split_first()?;
        let marker = match (opcode, operands) {
            (0x08, [0]) => Begin(Condition::Active),
            (0x39, [0]) => Begin(Condition::Flashed),
            (0x35, [0]) => Begin(Condition::Enraged),
            (0x54, [0]) => Begin(Condition::TargetAvailable),
            (0x02, [0]) => Begin(Condition::CheckTrackedPlayers),
            (0x78, [0, min, max]) if min <= max => Begin(Condition::TargetAngleIn {
                min: Degrees::from_native(*min),
                max: Degrees::from_native(*max),
            }),
            (0x0b, [0, value]) => match Mode::from_native(*value) {
                Some(mode) => Begin(Condition::ModeIs(mode)),
                None => Unsupported,
            },
            (opcode, [1]) if Self::is_condition_opcode(opcode) => Else,
            (opcode, [2]) if Self::is_condition_opcode(opcode) => End,
            (opcode, _) if Self::is_condition_opcode(opcode) => Unsupported,
            _ => return None,
        };
        Some(marker)
    }

    fn is_condition_opcode(opcode: u8) -> bool {
        matches!(
            opcode,
            0x02 | 0x08 | 0x0b | 0x1b | 0x35 | 0x39 | 0x54 | 0x78
        )
    }
}

pub(super) struct ConditionEncoding {
    pub begin: Vec<u8>,
    pub otherwise: &'static [u8],
    pub end: &'static [u8],
}

impl Condition {
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "active" => Some(Self::Active),
            "target_angle_in" => Some(Self::TargetAngleIn {
                min: Degrees::from_native(0),
                max: Degrees::from_native(0),
            }),
            "flashed" => Some(Self::Flashed),
            "enraged" => Some(Self::Enraged),
            "target.available" => Some(Self::TargetAvailable),
            "check_tracked_players" => Some(Self::CheckTrackedPlayers),
            "mode_is" => Some(Self::ModeIs(Mode::Normal)),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> String {
        match self {
            Self::Active => "self.active",
            Self::TargetAngleIn { min, max } => {
                return format!("self.target_angle_in({}, {})", min.value(), max.value());
            }
            Self::Flashed => "self.flashed",
            Self::Enraged => "self.enraged",
            Self::TargetAvailable => "self.target.available",
            Self::CheckTrackedPlayers => "self.check_tracked_players()",
            Self::ModeIs(value) => return format!("self.mode_is({})", value.name()),
        }
        .into()
    }

    /// Parameterized or effectful checks use method syntax.
    pub(super) fn is_method(self) -> bool {
        matches!(
            self,
            Self::CheckTrackedPlayers | Self::ModeIs(_) | Self::TargetAngleIn { .. }
        )
    }

    pub(super) fn encoding(self) -> ConditionEncoding {
        match self {
            Self::Active => ConditionEncoding {
                begin: vec![0x08, 0],
                otherwise: &[0x08, 1],
                end: &[0x08, 2],
            },
            Self::TargetAngleIn { min, max } => ConditionEncoding {
                begin: vec![0x78, 0, min.native(), max.native()],
                otherwise: &[0x78, 1],
                end: &[0x78, 2],
            },
            // 10861160 snapshots +2680 into +2600, then compares to the operand.
            Self::ModeIs(value) => ConditionEncoding {
                begin: vec![0x0b, 0x00, value as u8],
                otherwise: &[0x0b, 0x01],
                end: &[0x0b, 0x02],
            },
            // 10860E70: test tracked-player mask +2687; when empty, clear
            // current target +2612 to FF before entering the false branch.
            Self::CheckTrackedPlayers => ConditionEncoding {
                begin: vec![0x02, 0x00],
                otherwise: &[0x02, 0x01],
                end: &[0x02, 0x02],
            },
            // 10865450: selected player is active, not transitioning (+2042),
            // and in the same area as the monster. No target is false.
            Self::TargetAvailable => ConditionEncoding {
                begin: vec![0x54, 0x00],
                otherwise: &[0x54, 0x01],
                end: &[0x54, 0x02],
            },
            // 108635A0: execute the body iff the rage flag at +2726 is nonzero.
            Self::Enraged => ConditionEncoding {
                begin: vec![0x35, 0x00],
                otherwise: &[0x35, 0x01],
                end: &[0x35, 0x02],
            },
            // 10863750: execute the body iff the flash timer at +2914 is nonzero.
            Self::Flashed => ConditionEncoding {
                begin: vec![0x39, 0x00],
                otherwise: &[0x39, 0x01],
                end: &[0x39, 0x02],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_markers_decode_actual_operands() {
        use ConditionMarker::{Begin, Else, End};
        for condition in [
            Condition::Active,
            Condition::Flashed,
            Condition::Enraged,
            Condition::TargetAvailable,
            Condition::CheckTrackedPlayers,
            Condition::ModeIs(Mode::Normal),
            Condition::ModeIs(Mode::Attack),
            Condition::TargetAngleIn {
                min: Degrees::from_native(32),
                max: Degrees::from_native(200),
            },
        ] {
            let encoding = condition.encoding();
            assert_eq!(
                ConditionMarker::decode(&encoding.begin),
                Some(Begin(condition))
            );
            assert_eq!(ConditionMarker::decode(encoding.otherwise), Some(Else));
            assert_eq!(ConditionMarker::decode(encoding.end), Some(End));
        }
    }

    #[test]
    fn unsupported_conditions_are_distinct_from_other_instructions() {
        for bytes in [
            &[][..],
            &[0x92],
            &[0x79],
            &[0x79, 2],
            &[0x79, 3],
            &[0x79, 0, 1, 4, 0x79, 1, 1],
        ] {
            assert_eq!(ConditionMarker::decode(bytes), None);
        }
        for bytes in [
            &[0x08][..],
            &[0x35, 0, 1],
            &[0x39, 3],
            &[0x78, 0],
            &[0x78, 0, 200, 32],
            &[0x0b, 0],
            &[0x0b, 0, 2],
            &[0x1b, 0],
            &[0x1b, 0, 0],
            &[0x1b, 0, 2],
        ] {
            assert_eq!(
                ConditionMarker::decode(bytes),
                Some(ConditionMarker::Unsupported)
            );
        }
    }
}
