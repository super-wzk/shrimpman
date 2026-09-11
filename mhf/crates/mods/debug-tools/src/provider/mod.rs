//! Optional in-process tools for an offline session. Mutations run on the game thread.

mod input;
mod module;
mod monsters;
mod native;
mod overlay;
mod service;
mod ui;

use input::InputController;
#[cfg(test)]
use mhf_base::model::Face;
use mhf_base::model::{
    Appearance, AppearanceChange, AppearanceOptions, Equipment, NATIVE_WEAPON_NAMES, Transmogs,
};
pub use module::DebugToolsMod;
pub(crate) use native::{State, install};
pub(crate) use overlay::create as overlay;
pub use service::DebugService;
use std::sync::{Arc, Mutex, PoisonError};
use ui::DebugWindow;

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
