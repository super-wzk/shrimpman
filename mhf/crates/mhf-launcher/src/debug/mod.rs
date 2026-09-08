//! In-process tools for a temporary offline hunter. Native mutations run on the game thread.

mod monsters;
mod native;
mod quest;
mod ui;

pub(crate) use native::install;
use std::sync::{Arc, Mutex, PoisonError};
pub(crate) use ui::DebugWindow;

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

pub struct DebugSession {
    quest: quest::Quest,
    control: Arc<DebugControl>,
}

impl DebugSession {
    /// The embedded Chinese UTF-8 quest, starting at the Historical Site camp.
    pub fn test_map() -> Result<Self, String> {
        Ok(Self::from_quest(quest::Quest::test_map()?))
    }

    pub fn new(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self::from_quest(quest::Quest::parse(bytes)?))
    }

    fn from_quest(quest: quest::Quest) -> Self {
        let snapshot = DebugSnapshot {
            quest_id: quest.id,
            ..Default::default()
        };
        let control = Arc::new(DebugControl {
            shared: Mutex::new(Shared {
                snapshot,
                ..Default::default()
            }),
        });
        Self { quest, control }
    }

    pub(crate) fn control(&self) -> Arc<DebugControl> {
        self.control.clone()
    }
}

struct Equipment {
    kind: u8,
    id: u16,
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
    Equip { kind: u8, id: u16 },
    Action(Action),
    FollowEquipment,
    Transform(u8),
    TransformAction { species: u8, action: MonsterAction },
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
    actions: [Vec<Action>; 14],
    monsters: Vec<Monster>,
}

struct Monster {
    id: u8,
    name: &'static str,
    actions: Vec<MonsterAction>,
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
struct DebugSnapshot {
    quest_id: u16,
    ready: bool,
    scene: u8,
    area: u16,
    map: u16,
    areas: Vec<u16>,
    weapon: u8,
    equipped_weapon: u8,
    equipment: Vec<(u8, u16)>,
    action_group: u8,
    action_id: u8,
    action_stage: u8,
    animation: u16,
    frame: f32,
    position: [f32; 3],
    message: String,
    catalog: Arc<Catalog>,
    monster: Option<u8>,
    controlling_monster: bool,
    monster_actions: Vec<MonsterAction>,
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

pub(crate) struct DebugControl {
    shared: Mutex<Shared>,
}

impl DebugControl {
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

    fn snapshot(&self) -> DebugSnapshot {
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
