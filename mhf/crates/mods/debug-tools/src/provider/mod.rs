//! Optional in-process tools for an offline session. Mutations run on the game thread.

mod input;
mod module;
mod monsters;
mod native;
mod overlay;
mod service;
mod ui;

use input::InputController;
pub use module::DebugToolsMod;
pub(crate) use native::{State, install};
pub(crate) use overlay::create as overlay;
pub use service::DebugService;
use std::sync::{Arc, Mutex, PoisonError};
use ui::DebugWindow;

// Native DAT class IDs (melee record +3, ranged record +4) and player +3.
// These differ from the server's character::WeaponType discriminants.
const NATIVE_WEAPON_NAMES: [&str; 14] = [
    "大剑",
    "重弩",
    "大锤",
    "长枪",
    "片手剑",
    "轻弩",
    "双剑",
    "太刀",
    "狩猎笛",
    "铳枪",
    "弓",
    "穿龙棍",
    "斩斧 F",
    "磁斩锤",
];

struct Equipment {
    kind: u8,
    id: u16,
    model_ids: [u16; 2],
    weapon: Option<u8>,
    name: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Action {
    group: u8,
    id: u8,
    weapon: u8,
}

impl Action {
    fn label(self) -> String {
        match (self.group, self.id) {
            (0, 0) => "待机".into(),
            (0, _) => format!("通用动作 {:03}", self.id),
            _ => format!("武器招式 {:03}", self.id),
        }
    }
}

enum DebugCommand {
    Equip {
        kind: u8,
        id: u16,
    },
    Transmog {
        kind: u8,
        id: Option<u16>,
    },
    Appearance(AppearanceChange),
    Action(Action),
    FollowEquipment,
    Transform {
        species: u8,
        variant: u8,
    },
    TransformAction {
        species: u8,
        variant: u8,
        action: MonsterAction,
    },
    RestoreHunter,
    MonsterAction(MonsterAction),
    NextMonsterAction,
    CameraDistance(f32),
    CameraPitch(f32),
    ChangeArea(u16),
    Restart,
    Exit,
}

#[derive(Default)]
struct Catalog {
    equipment: Vec<Equipment>,
    appearances: [AppearanceOptions; 2],
    actions: [Vec<Action>; 14],
    monsters: Vec<Monster>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Transmogs {
    // Native armor kinds; kind 1 is the face, which has no armor override.
    armor: [u16; 6],
}

impl Transmogs {
    fn changed(
        mut self,
        kind: u8,
        id: Option<u16>,
        catalog: &Catalog,
    ) -> Result<Self, &'static str> {
        if !matches!(kind, 0 | 2..=5) {
            return Err("此部位不支持防具幻化");
        }
        if let Some(id) = id
            && (id == 0
                || !catalog
                    .equipment
                    .iter()
                    .any(|item| item.kind == kind && item.id == id && item.weapon.is_none()))
        {
            return Err("幻化防具编号无效");
        }
        self.armor[usize::from(kind)] = id.unwrap_or(0);
        Ok(self)
    }

    fn selected(&self, kind: u8) -> Option<u16> {
        self.armor
            .get(usize::from(kind))
            .copied()
            .filter(|&id| id != 0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Appearance {
    female: bool,
    face: u8,
    hair: u8,
}

enum AppearanceChange {
    Gender(bool),
    Face(u8),
    Hair(u8),
}

#[derive(Default)]
struct AppearanceOptions {
    faces: Vec<Face>,
    hair: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
struct Face {
    id: u8,
    model_id: u16,
}

impl Appearance {
    fn changed(
        mut self,
        change: AppearanceChange,
        options: &[AppearanceOptions; 2],
    ) -> Result<Self, &'static str> {
        match change {
            AppearanceChange::Gender(female) => {
                let options = &options[usize::from(female)];
                let face = options.faces.first().ok_or("此性别没有可用脸型")?.id;
                let hair = *options.hair.first().ok_or("此性别没有可用发型")?;
                self.female = female;
                if !options.faces.iter().any(|face| face.id == self.face) {
                    self.face = face;
                }
                if !options.hair.contains(&self.hair) {
                    self.hair = hair;
                }
            }
            AppearanceChange::Face(face) => {
                if !options[usize::from(self.female)]
                    .faces
                    .iter()
                    .any(|option| option.id == face)
                {
                    return Err("脸型编号无效");
                }
                self.face = face;
            }
            AppearanceChange::Hair(hair) => {
                if !options[usize::from(self.female)].hair.contains(&hair) {
                    return Err("发型编号无效");
                }
                self.hair = hair;
            }
        }
        Ok(self)
    }
}

struct Monster {
    id: u8,
    name: &'static str,
    variants: Vec<monsters::Variant>,
    actions: Arc<Vec<MonsterAction>>,
}

impl Monster {
    fn variant(&self, id: u8) -> Option<monsters::Variant> {
        self.variants
            .iter()
            .find(|variant| variant.id == id)
            .copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct MonsterAction {
    group: u8,
    id: u8,
}

impl MonsterAction {
    fn label(self) -> String {
        format!("怪物招式 {}:{:03}", self.group, self.id)
    }
}

#[derive(Clone, Copy, Default)]
struct MonsterInput {
    forward: f32,
    sideways: f32,
    vertical: f32,
    speed: f32,
}

#[derive(Clone, Default)]
struct CombatSnapshot {
    checks: u32,
    hits: u32,
    health: Vec<MonsterHealth>,
    last_damage: String,
}

#[derive(Clone)]
struct MonsterHealth {
    slot: u16,
    species: u8,
    hp: i32,
    controlled: bool,
}

#[derive(Clone, Default)]
pub(crate) struct DebugSnapshot {
    quest_id: u16,
    ready: bool,
    scene: u8,
    area: u16,
    map: u16,
    areas: Vec<u16>,
    weapon: u8,
    equipped_weapon: u8,
    equipment: [Option<(u8, u16)>; 6],
    transmogs: Transmogs,
    appearance: Appearance,
    action_group: u8,
    action_id: u8,
    action_stage: u8,
    animation: u16,
    frame: f32,
    position: [f32; 3],
    message: Arc<str>,
    catalog: Arc<Catalog>,
    monster: Option<u8>,
    monster_variant: u8,
    controlling_monster: bool,
    monster_actions: Option<Arc<Vec<MonsterAction>>>,
    camera_distance: f32,
    camera_pitch: f32,
    combat: CombatSnapshot,
}

#[derive(Default)]
struct Shared {
    snapshot: DebugSnapshot,
    commands: Vec<DebugCommand>,
    monster_input: MonsterInput,
    input_at: Option<std::time::Instant>,
}

pub struct DebugControl {
    shared: Mutex<Shared>,
}

impl DebugControl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            shared: Mutex::new(Shared::default()),
        })
    }

    fn set_monster_input(&self, input: MonsterInput) {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        shared.monster_input = input;
        shared.input_at = Some(std::time::Instant::now());
    }

    fn monster_input(&self) -> MonsterInput {
        let shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if shared
            .input_at
            .is_some_and(|at| at.elapsed().as_millis() < 200)
        {
            shared.monster_input
        } else {
            MonsterInput::default()
        }
    }

    pub(crate) fn snapshot(&self) -> DebugSnapshot {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot
            .clone()
    }
    fn send(&self, command: DebugCommand) -> Result<(), String> {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if shared.commands.len() >= 16 {
            return Err("等待上一个调试操作完成".into());
        }
        shared.commands.push(command);
        Ok(())
    }
    pub(crate) fn mod_snapshot(&self) -> crate::api::Snapshot {
        let shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        let snapshot = &shared.snapshot;
        crate::api::Snapshot {
            ready: snapshot.ready,
            quest_id: snapshot.quest_id,
            area: snapshot.area,
            monster: snapshot.monster.into(),
            position: snapshot.position,
            hit_checks: snapshot.combat.checks,
            hits: snapshot.combat.hits,
        }
    }

    pub(crate) fn mod_command(&self, command: crate::api::Command) -> Result<(), String> {
        use crate::api::Command;
        self.send(match command {
            Command::Transform(species) => DebugCommand::Transform {
                species,
                variant: 0,
            },
            Command::RestoreHunter => DebugCommand::RestoreHunter,
            Command::ChangeArea(area) => DebugCommand::ChangeArea(area),
            Command::Restart => DebugCommand::Restart,
            Command::Exit => DebugCommand::Exit,
        })
    }

    fn commands(&self) -> Vec<DebugCommand> {
        std::mem::take(
            &mut self
                .shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .commands,
        )
    }
    fn publish(&self, snapshot: DebugSnapshot) {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot = snapshot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transmog_changes_and_restores_only_the_selected_armor_slot() {
        let catalog = Catalog {
            equipment: [0, 2, 3, 4, 5]
                .into_iter()
                .map(|kind| Equipment {
                    kind,
                    id: 17,
                    model_ids: [20, 30],
                    weapon: None,
                    name: String::new(),
                })
                .collect(),
            ..Default::default()
        };
        let mut transmogs = Transmogs::default();
        for kind in [0, 2, 3, 4, 5] {
            transmogs = transmogs.changed(kind, Some(17), &catalog).unwrap();
        }
        assert_eq!(transmogs.armor, [17, 0, 17, 17, 17, 17]);
        let restored = transmogs.changed(3, None, &catalog).unwrap();
        assert_eq!(restored.armor, [17, 0, 17, 0, 17, 17]);
        assert_eq!(transmogs.armor[3], 17);
    }

    #[test]
    fn transmog_rejects_invalid_slots_zero_unknown_and_wrong_slot_ids() {
        let catalog = Catalog {
            equipment: vec![
                Equipment {
                    kind: 2,
                    id: 10,
                    model_ids: [20, 30],
                    weapon: None,
                    name: String::new(),
                },
                Equipment {
                    kind: 6,
                    id: 11,
                    model_ids: [40; 2],
                    weapon: Some(0),
                    name: String::new(),
                },
            ],
            ..Default::default()
        };
        let transmogs = Transmogs::default();
        for (kind, id) in [
            (1, Some(10)),
            (6, Some(11)),
            (7, None),
            (u8::MAX, None),
            (2, Some(0)),
            (2, Some(11)),
            (3, Some(10)),
        ] {
            assert!(transmogs.changed(kind, id, &catalog).is_err());
        }
        assert_eq!(transmogs.changed(2, None, &catalog), Ok(transmogs));
    }

    fn appearance_options() -> [AppearanceOptions; 2] {
        [
            AppearanceOptions {
                faces: (0..3)
                    .map(|id| Face {
                        id,
                        model_id: u16::from(id) + 10,
                    })
                    .collect(),
                hair: vec![0, 27, 150],
            },
            AppearanceOptions {
                faces: (0..2)
                    .map(|id| Face {
                        id,
                        model_id: u16::from(id) + 10,
                    })
                    .collect(),
                hair: vec![0, 27],
            },
        ]
    }

    #[test]
    fn appearance_commands_preserve_other_fields_and_use_the_current_gender() {
        let options = appearance_options();
        let original = Appearance {
            female: true,
            ..Default::default()
        };
        let next = original
            .changed(AppearanceChange::Face(1), &options)
            .unwrap()
            .changed(AppearanceChange::Hair(27), &options)
            .unwrap();
        assert_eq!(
            next,
            Appearance {
                female: true,
                face: 1,
                hair: 27
            }
        );
        assert!(next.changed(AppearanceChange::Face(2), &options).is_err());
        assert!(next.changed(AppearanceChange::Hair(150), &options).is_err());
        assert!(next.changed(AppearanceChange::Hair(17), &options).is_err());
    }

    #[test]
    fn gender_change_retains_supported_styles_and_replaces_missing_styles() {
        let options = appearance_options();
        for (face, hair, expected) in [(1, 27, (1, 27)), (2, 150, (0, 0))] {
            let original = Appearance {
                face,
                hair,
                ..Default::default()
            };
            let next = original
                .changed(AppearanceChange::Gender(true), &options)
                .unwrap();
            assert!(next.female);
            assert_eq!((next.face, next.hair), expected);
            assert_eq!(
                next.changed(AppearanceChange::Gender(false), &options)
                    .unwrap(),
                Appearance {
                    female: false,
                    ..next
                }
            );
        }
    }

    #[test]
    fn gender_change_requires_both_appearance_directories() {
        let mut options = appearance_options();
        options[1].hair.clear();
        assert!(
            Appearance::default()
                .changed(AppearanceChange::Gender(true), &options)
                .is_err()
        );
        options[1].hair.push(0);
        options[1].faces.clear();
        assert!(
            Appearance::default()
                .changed(AppearanceChange::Gender(true), &options)
                .is_err()
        );
    }
}
