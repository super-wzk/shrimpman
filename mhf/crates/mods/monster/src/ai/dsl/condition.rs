//! Semantic conditions and their verified native control-flow encodings.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    Active,
    Airborne,
    InArea(u16),
    NearTarget2d(u8),
    NearTarget3d(u8),
    InAction(u8, u8),
    TargetAngleAtLeast(u8),
    TargetAngleIn { min: Degrees, max: Degrees },
    Flashed,
    Enraged,
    AreaTimerExpired,
    AttackTimerActive,
    AnyPlayerCarrying,
    IsDaytime,
    HasPlayerInSameArea,
    TargetAvailable,
    TargetDetected,
    TargetGroundIs(u8),
    TargetPositionAvailable,
    CheckTrackedPlayers,
    CheckPendingArea,
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
            (0x09, [0]) => Begin(Condition::Airborne),
            (0x0e, [0, hi, lo]) => Begin(Condition::InArea(u16::from_be_bytes([*hi, *lo]))),
            (0x14, [0, degrees]) => Begin(Condition::TargetAngleAtLeast(*degrees)),
            (0x34, [0, group, id]) => Begin(Condition::InAction(*group, *id)),
            (0x22, [0, value]) => Begin(Condition::NearTarget2d(*value)),
            (0x36, [0, value]) => Begin(Condition::NearTarget3d(*value)),
            (0x39, [0]) => Begin(Condition::Flashed),
            (0x35, [0]) => Begin(Condition::Enraged),
            (0x29, [0]) => Begin(Condition::AreaTimerExpired),
            (0x2a, [0]) => Begin(Condition::AttackTimerActive),
            (0x28, [0]) => Begin(Condition::HasPlayerInSameArea),
            (0x2f, [0]) => Begin(Condition::AnyPlayerCarrying),
            (0x77, [0]) => Begin(Condition::IsDaytime),
            (0x4a, [0]) => Begin(Condition::TargetDetected),
            (0x54, [0]) => Begin(Condition::TargetAvailable),
            (0x5a, [0, value]) => Begin(Condition::TargetGroundIs(*value)),
            (0x5d, [0]) => Begin(Condition::TargetPositionAvailable),
            (0x02, [0]) => Begin(Condition::CheckTrackedPlayers),
            (0x03, [0]) => Begin(Condition::CheckPendingArea),
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
            0x02 | 0x03
                | 0x08
                | 0x09
                | 0x0b
                | 0x0e
                | 0x14
                | 0x22
                | 0x28
                | 0x1b
                | 0x29
                | 0x2a
                | 0x2f
                | 0x34
                | 0x35
                | 0x36
                | 0x39
                | 0x4a
                | 0x54
                | 0x5a
                | 0x5d
                | 0x77
                | 0x78
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
            "airborne" => Some(Self::Airborne),
            "in_area" => Some(Self::InArea(0)),
            "in_action" => Some(Self::InAction(0, 0)),
            "near_target_2d" => Some(Self::NearTarget2d(0)),
            "near_target_3d" => Some(Self::NearTarget3d(0)),
            "target_angle_at_least" => Some(Self::TargetAngleAtLeast(0)),
            "target_angle_in" => Some(Self::TargetAngleIn {
                min: Degrees::from_native(0),
                max: Degrees::from_native(0),
            }),
            "flashed" => Some(Self::Flashed),
            "enraged" => Some(Self::Enraged),
            "area_timer_expired" => Some(Self::AreaTimerExpired),
            "attack_timer_active" => Some(Self::AttackTimerActive),
            "has_player_in_same_area" => Some(Self::HasPlayerInSameArea),
            "target_detected" => Some(Self::TargetDetected),
            "target_available" => Some(Self::TargetAvailable),
            "target_ground_is" => Some(Self::TargetGroundIs(0)),
            "target_position_available" => Some(Self::TargetPositionAvailable),
            "check_tracked_players" => Some(Self::CheckTrackedPlayers),
            "check_pending_area" => Some(Self::CheckPendingArea),
            "mode_is" => Some(Self::ModeIs(Mode::Normal)),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> String {
        match self {
            Self::Active => "self.active",
            Self::Airborne => "self.airborne",
            Self::InAction(group, id) => return format!("self.in_action({group}:{id})"),
            Self::NearTarget2d(value) => return format!("self.near_target_2d({value})"),
            Self::NearTarget3d(value) => return format!("self.near_target_3d({value})"),
            Self::InArea(area) => return format!("self.in_area({area})"),
            Self::TargetAngleAtLeast(degrees) => {
                return format!("self.target_angle_at_least({degrees})");
            }
            Self::TargetAngleIn { min, max } => {
                return format!("self.target_angle_in({}, {})", min.value(), max.value());
            }
            Self::Flashed => "self.flashed",
            Self::Enraged => "self.enraged",
            Self::AreaTimerExpired => "self.area_timer_expired",
            Self::AttackTimerActive => "self.attack_timer_active",
            Self::AnyPlayerCarrying => "context.any_player_carrying()",
            Self::IsDaytime => "context.is_daytime",
            Self::HasPlayerInSameArea => "self.has_player_in_same_area()",
            Self::TargetDetected => "self.target_detected",
            Self::TargetAvailable => "self.target_available()",
            Self::TargetGroundIs(value) => return format!("self.target_ground_is({value})"),
            Self::TargetPositionAvailable => "self.target_position_available()",
            Self::CheckTrackedPlayers => "self.check_tracked_players()",
            Self::CheckPendingArea => "self.check_pending_area()",
            Self::ModeIs(value) => return format!("self.mode_is({})", value.name()),
        }
        .into()
    }

    /// Queries, parameterized checks, and effectful checks use method syntax.
    pub(super) fn is_method(self) -> bool {
        matches!(
            self,
            Self::CheckTrackedPlayers
                | Self::HasPlayerInSameArea
                | Self::AnyPlayerCarrying
                | Self::TargetAvailable
                | Self::CheckPendingArea
                | Self::TargetPositionAvailable
                | Self::TargetGroundIs(_)
                | Self::ModeIs(_)
                | Self::InArea(_)
                | Self::NearTarget2d(_)
                | Self::NearTarget3d(_)
                | Self::InAction(_, _)
                | Self::TargetAngleAtLeast(_)
                | Self::TargetAngleIn { .. }
        )
    }

    pub(super) fn encoding(self) -> ConditionEncoding {
        match self {
            Self::InArea(area) => {
                let [hi, lo] = area.to_be_bytes();
                ConditionEncoding {
                    begin: vec![0x0e, 0, hi, lo],
                    otherwise: &[0x0e, 1],
                    end: &[0x0e, 2],
                }
            }
            Self::InAction(group, id) => ConditionEncoding {
                begin: vec![0x34, 0, group, id],
                otherwise: &[0x34, 1],
                end: &[0x34, 2],
            },
            // 10862700: full 3D distance, no body-size floor.
            Self::NearTarget3d(value) => ConditionEncoding {
                begin: vec![0x36, 0x00, value],
                otherwise: &[0x36, 0x01],
                end: &[0x36, 0x02],
            },
            // 108625B0: horizontal (x/z) distance with a body-size floor.
            Self::NearTarget2d(value) => ConditionEncoding {
                begin: vec![0x22, 0, value],
                otherwise: &[0x22, 1],
                end: &[0x22, 2],
            },
            Self::Airborne => ConditionEncoding {
                begin: vec![0x09, 0],
                otherwise: &[0x09, 1],
                end: &[0x09, 2],
            },
            Self::Active => ConditionEncoding {
                begin: vec![0x08, 0],
                otherwise: &[0x08, 1],
                end: &[0x08, 2],
            },
            // 108618F0: a bound player/monster target must have an absolute
            // horizontal angle >= the integer-degree operand (not 0x78's scale).
            Self::TargetAngleAtLeast(degrees) => ConditionEncoding {
                begin: vec![0x14, 0, degrees],
                otherwise: &[0x14, 1],
                end: &[0x14, 2],
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
            // 10860EF0: require a different, non-FFFF, allowed pending area.
            // Failure clears both command tuples; a rejected area becomes current.
            Self::CheckPendingArea => ConditionEncoding {
                begin: vec![0x03, 0],
                otherwise: &[0x03, 1],
                end: &[0x03, 2],
            },
            // 108646B0 reads the existing detection bit; it does not rerun sight checks.
            Self::TargetDetected => ConditionEncoding {
                begin: vec![0x4a, 0],
                otherwise: &[0x4a, 1],
                end: &[0x4a, 2],
            },
            // 10865450: selected player is active, not transitioning (+2042),
            // and in the same area as the monster. No target is false.
            Self::TargetAvailable => ConditionEncoding {
                begin: vec![0x54, 0x00],
                otherwise: &[0x54, 0x01],
                end: &[0x54, 0x02],
            },
            // 10865AE0 requires saved kind 1/11 and a selected player, then
            // compares the low byte of its area-adapted ground number.
            Self::TargetGroundIs(value) => ConditionEncoding {
                begin: vec![0x5a, 0, value],
                otherwise: &[0x5a, 1],
                end: &[0x5a, 2],
            },
            // 10865CF0: resolves the current target once, then enters the body only
            // when all three reference-point components +2852/+2856/+2860 are nonzero.
            Self::TargetPositionAvailable => ConditionEncoding {
                begin: vec![0x5d, 0x00],
                otherwise: &[0x5d, 0x01],
                end: &[0x5d, 0x02],
            },
            // 10862F40: execute the body iff signed i16 +2910 is <= 0.
            Self::AreaTimerExpired => ConditionEncoding {
                begin: vec![0x29, 0],
                otherwise: &[0x29, 1],
                end: &[0x29, 2],
            },
            // 10862FC0: checks signed i16 +2912, not the current attack mode.
            Self::AttackTimerActive => ConditionEncoding {
                begin: vec![0x2a, 0],
                otherwise: &[0x2a, 1],
                end: &[0x2a, 2],
            },
            // 108632F0: any configured player has a nonzero carry-state low nibble.
            // No area, distance, item-category, or target filter is applied.
            Self::AnyPlayerCarrying => ConditionEncoding {
                begin: vec![0x2f, 0],
                otherwise: &[0x2f, 1],
                end: &[0x2f, 2],
            },
            // 108665F0: day bit set OR night bit clear, including neither bit set.
            Self::IsDaytime => ConditionEncoding {
                begin: vec![0x77, 0],
                otherwise: &[0x77, 1],
                end: &[0x77, 2],
            },
            // 10862E40: some active player record shares the monster's raw
            // area id at +2040. No tracking, distance, or mapping is applied.
            Self::HasPlayerInSameArea => ConditionEncoding {
                begin: vec![0x28, 0],
                otherwise: &[0x28, 1],
                end: &[0x28, 2],
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
            Condition::AreaTimerExpired,
            Condition::AttackTimerActive,
            Condition::AnyPlayerCarrying,
            Condition::IsDaytime,
            Condition::HasPlayerInSameArea,
            Condition::TargetAvailable,
            Condition::TargetDetected,
            Condition::TargetGroundIs(0),
            Condition::TargetGroundIs(255),
            Condition::TargetPositionAvailable,
            Condition::CheckTrackedPlayers,
            Condition::CheckPendingArea,
            Condition::NearTarget2d(5),
            Condition::NearTarget3d(7),
            Condition::ModeIs(Mode::Normal),
            Condition::ModeIs(Mode::Attack),
            Condition::TargetAngleAtLeast(0),
            Condition::TargetAngleAtLeast(255),
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
            &[0x03],
            &[0x03, 0, 1],
            &[0x03, 1, 0],
            &[0x03, 2, 0],
            &[0x03, 3],
            &[0x35, 0, 1],
            &[0x39, 3],
            &[0x29],
            &[0x29, 3],
            &[0x29, 0, 1],
            &[0x2a],
            &[0x2a, 3],
            &[0x2a, 0, 1],
            &[0x2f],
            &[0x2f, 3],
            &[0x2f, 0, 1],
            &[0x28],
            &[0x28, 3],
            &[0x28, 0, 1],
            &[0x77],
            &[0x77, 3],
            &[0x77, 0, 1],
            &[0x78, 0],
            &[0x78, 0, 200, 32],
            &[0x14],
            &[0x14, 0],
            &[0x14, 0, 1, 2],
            &[0x14, 1, 0],
            &[0x14, 2, 0],
            &[0x14, 3],
            &[0x5a],
            &[0x5a, 0],
            &[0x5a, 0, 1, 2],
            &[0x5a, 1, 0],
            &[0x5a, 2, 0],
            &[0x5a, 3],
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
