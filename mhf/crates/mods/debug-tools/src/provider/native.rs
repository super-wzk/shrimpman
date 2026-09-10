mod area;
mod combat;
mod equipment;
mod monster;

use super::{Action, Catalog, DebugCommand, DebugControl, DebugSnapshot, Equipment};
use mhf_hooks::{HookGuard, HookSlot, ModuleReference};
use mhf_quest::QuestControl;
use std::{
    ffi::c_void,
    mem::transmute,
    ptr,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU32, AtomicUsize, Ordering},
    },
};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Globalization::{MB_ERR_INVALID_CHARS, MultiByteToWideChar};

static SLOT: HookSlot<State> = HookSlot::new();
static BASE: AtomicUsize = AtomicUsize::new(0);

type HurtboxCopies = std::collections::HashMap<(usize, usize), Box<[[u8; 40]]>>;

pub(crate) struct State {
    // Release explicitly after detach; retain native-facing buffers through DllMain.
    module: ModuleReference,
    base: usize,
    session: QuestControl<'static>,
    quest_id: u16,
    control: Arc<DebugControl>,
    dispatch: usize,
    initialize_players: usize,
    monster_ai: usize,
    hunter_update: usize,
    camera_update: usize,
    render_world: usize,
    collide_effects: usize,
    hit_target: usize,
    hurt_shapes: usize,
    combat: Mutex<combat::Diagnostics>,
    hurtbox_copies: Mutex<HurtboxCopies>,
    camera_distance: AtomicU32,
    camera_pitch: AtomicU32,
    controlled_monster: AtomicUsize,
    runtime: Mutex<Runtime>,
}

#[derive(Default)]
struct Runtime {
    catalog: Arc<Catalog>,
    catalog_ready: bool,
    message: String,
    moveset: Option<u8>,
    pending_action: Option<Action>,
    ready_frames: u8,
    monster: Option<monster::Control>,
}

impl State {
    /// Call after all hooks detach, retaining this state through final DLL unload.
    pub(crate) unsafe fn prepare_release(&mut self) -> Result<(), String> {
        unsafe { self.module.release() }
    }

    fn address(&self, va: usize) -> usize {
        self.base + va - 0x1000_0000
    }
    unsafe fn read<T: Copy>(&self, va: usize) -> T {
        unsafe { get(self.address(va)) }
    }
    unsafe fn write<T>(&self, va: usize, value: T) {
        unsafe { put(self.address(va), value) }
    }
}

unsafe fn get<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_unaligned(address as *const T) }
}
unsafe fn put<T>(address: usize, value: T) {
    unsafe { ptr::write_unaligned(address as *mut T, value) }
}

// The offline bootstrap and quest transport entrypoints are deliberately absent.
const SIGNATURES: &[(usize, &[u8])] = &[
    (0x008fcee0, &[0x55, 0x8b, 0xec, 0x51, 0x0f, 0xb7, 0x05]),
    (
        0x0089e510,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x1c, 0x53, 0x33],
    ),
    (
        0x008696d0,
        &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x83, 0xec],
    ),
    (0x00a5b800, &[0x55, 0x8b, 0xec, 0x51, 0xa1]),
    (
        0x00baee10,
        &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf0, 0x83, 0xec, 0x68],
    ),
    (0x00aa4470, &[0x56, 0x8b, 0x35]),
    (0x00b65fd0, &[0x8a, 0x48, 0x03, 0x80, 0xe1, 0x03]),
    (0x00b4b9f0, &[0x53, 0x8b, 0x1d]),
    (
        0x00a94d25,
        &[0x66, 0x83, 0x78, 0x08, 0x00, 0x53, 0x8b, 0x58, 0x6c, 0x57],
    ),
    (0x00aaa420, &[0x55, 0x8b, 0xec, 0x53, 0x56, 0x57]),
    (0x00b7b570, &[0x56, 0xe8, 0x5a, 0xf7, 0xff, 0xff]),
    (0x008b6bf0, &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x24]),
    (0x008b7b60, &[0x55, 0x8b, 0xec, 0x80, 0x3e, 0x00]),
    (0x00846ca0, &[0x55, 0x8b, 0xec, 0x56, 0x8b, 0xf0]),
    (0x00aa0d70, &[0x0f, 0xb7, 0x8a, 0x24, 0x06, 0x00, 0x00]),
    (
        0x00ba7160,
        &[0x0f, 0xb7, 0xd0, 0x03, 0xd2, 0xf6, 0x84, 0xd1],
    ),
    (
        0x00b9f080,
        &[0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d, 0x0c, 0x56],
    ),
    (
        0x00a89cd0,
        &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x81, 0xec],
    ),
    (
        0x001c0780,
        &[0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d, 0x08, 0x56],
    ),
    (
        0x00b37680,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x20, 0x53, 0x56],
    ),
    (
        0x00b9f820,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x53, 0x8b],
    ),
    (
        0x008f9960,
        &[0x56, 0x8d, 0xb7, 0x00, 0x04, 0x00, 0x00, 0x6a],
    ),
    (
        0x008fb7c0,
        &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x88, 0x00, 0x00],
    ),
    (
        0x008fca00,
        &[0x55, 0x8b, 0xec, 0x83, 0x7d, 0x08, 0x00, 0x57],
    ),
    (
        0x0089f8c0,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x53, 0x56],
    ),
    (
        0x00a92d70,
        &[0x55, 0x8b, 0xec, 0x51, 0x85, 0xf6, 0x74, 0x3b],
    ),
    // Skip the relocated absolute address in the first MOVSS instruction.
    (
        0x008ec098,
        &[0x0f, 0x57, 0xc0, 0x53, 0x56, 0x57, 0x8b, 0xf8],
    ),
    (
        0x00bba300,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x56, 0x57],
    ),
];

unsafe fn validate(base: usize) -> Result<(), String> {
    for &(rva, expected) in SIGNATURES {
        if unsafe { std::slice::from_raw_parts((base + rva) as *const u8, expected.len()) }
            != expected
        {
            return Err(format!("不支持此游戏 DLL 的调试接口：RVA {rva:#x}"));
        }
    }
    Ok(())
}

/// Stop all native callbacks before uninstalling this group, then uninstall
/// the offline group. Both guards retain the DLL and native-facing allocations.
pub(crate) unsafe fn install(
    module: HMODULE,
    session: QuestControl<'static>,
    control: Arc<DebugControl>,
) -> Result<HookGuard<State>, String> {
    let game_module =
        unsafe { mhf_mod_sdk::host::game_module_from_raw(module.0) }.ok_or("游戏模块尚未加载")?;
    unsafe { session.validate_module(game_module) }.map_err(|error| error.to_string())?;
    let quest_id = session.snapshot().quest_id;
    let base = module.0 as usize;
    unsafe { validate(base) }?;
    let retained = unsafe { ModuleReference::acquire(module) }?;
    BASE.store(base, Ordering::Relaxed);
    let mut hooks = SLOT.prepare()?;
    let dispatch = unsafe {
        hooks.create(
            "debug commands",
            (base + 0x008fcee0) as _,
            dispatch as *mut c_void,
        )
    }?;
    let initialize_players = unsafe {
        hooks.create(
            "debug moveset",
            (base + 0x0089e510) as _,
            initialize_players as *mut c_void,
        )
    }?;
    let monster_ai = unsafe {
        hooks.create(
            "debug monster AI",
            (base + 0x008696d0) as _,
            monster::select_action as *mut c_void,
        )
    }?;
    let hunter_update = unsafe {
        hooks.create(
            "debug hunter suspension",
            (base + 0x00a5b800) as _,
            monster::update_hunters as *mut c_void,
        )
    }?;
    let camera_update = unsafe {
        hooks.create(
            "debug monster camera",
            (base + 0x00baee10) as _,
            monster::update_camera as *mut c_void,
        )
    }?;
    let render_world = unsafe {
        hooks.create(
            "debug hunter model visibility",
            (base + 0x00b7b570) as _,
            combat::render_world as *mut c_void,
        )
    }?;
    let collide_effects = unsafe {
        hooks.create(
            "debug monster attack targets",
            (base + 0x008b6bf0) as _,
            combat::collide_effects as *mut c_void,
        )
    }?;
    let hit_target = unsafe {
        hooks.create(
            "debug monster damage recipient",
            (base + 0x008b7b60) as _,
            combat::hit_target as *mut c_void,
        )
    }?;
    let hurt_shapes = unsafe {
        hooks.create(
            "debug hostile monster hurtboxes",
            (base + 0x00846ca0) as _,
            combat::hurt_shapes as *mut c_void,
        )
    }?;
    control.publish(DebugSnapshot {
        quest_id,
        ..Default::default()
    });
    unsafe {
        hooks.install(State {
            module: retained,
            base,
            session,
            quest_id,
            control,
            dispatch: dispatch as usize,
            initialize_players: initialize_players as usize,
            monster_ai: monster_ai as usize,
            hunter_update: hunter_update as usize,
            camera_update: camera_update as usize,
            render_world: render_world as usize,
            collide_effects: collide_effects as usize,
            hit_target: hit_target as usize,
            hurt_shapes: hurt_shapes as usize,
            combat: Mutex::new(combat::Diagnostics::default()),
            hurtbox_copies: Mutex::new(std::collections::HashMap::new()),
            camera_distance: AtomicU32::new(2200.0_f32.to_bits()),
            camera_pitch: AtomicU32::new(60.0_f32.to_bits()),
            controlled_monster: AtomicUsize::new(0),
            runtime: Mutex::new(Runtime::default()),
        })
    }
}

unsafe extern "C" fn initialize_players() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x0089e510) };
        return unsafe { original() };
    };
    unsafe {
        let original: unsafe extern "C" fn() -> i32 = transmute(state.initialize_players);
        let result = original();
        let runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(weapon) = runtime.moveset
            && let Some(player) = monster::hunter(state)
            && get::<u8>(player) != 0
        {
            // 1089DF70 loads the moveset after this constructor returns.
            // Keep the save, equipment records and model IDs untouched.
            put(player + 3, weapon);
        }
        result
    }
}

unsafe fn restart(state: &State, runtime: &mut Runtime) -> Result<(), String> {
    unsafe {
        monster::release(state, runtime);
        *state.combat.lock().unwrap_or_else(PoisonError::into_inner) = Default::default();
        if let Some(control) = &mut runtime.monster {
            control.waiting = true;
            control.resume_at_arrival = false;
            control.instance = None;
        }
        state.session.restart().map_err(|error| error.to_string())
    }
}

unsafe fn text(pointer: usize) -> Option<String> {
    if pointer == 0 {
        return None;
    }
    // All catalog pointers come from the supported, relocated DAT resource.
    let mut length = 0;
    while length < 1024 && unsafe { get::<u8>(pointer + length) } != 0 {
        length += 1;
    }
    if length == 0 || length == 1024 {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer as *const u8, length) };
    decode_catalog_text(bytes)
}

fn decode_catalog_text(bytes: &[u8]) -> Option<String> {
    // Catalog strings in the supported Japanese DAT use CP932.
    let length = unsafe { MultiByteToWideChar(932, MB_ERR_INVALID_CHARS, bytes, None) };
    if length <= 0 {
        return None;
    }
    let mut utf16 = vec![0; length as usize];
    let written =
        unsafe { MultiByteToWideChar(932, MB_ERR_INVALID_CHARS, bytes, Some(&mut utf16)) };
    if written != length {
        return None;
    }
    String::from_utf16(&utf16).ok()
}

unsafe fn catalog(state: &State) -> Catalog {
    let mut catalog = Catalog::default();
    unsafe {
        catalog.monsters = super::monsters::NAMES
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(id, _)| state.read::<u32>(0x118c3628 + id * 4) != 0)
            .map(|(id, name)| super::Monster {
                id: id as u8,
                name,
                actions: super::monsters::actions(id as u8),
            })
            .collect();
        let dat = state.read::<usize>(0x1e77dcc4);
        // Name-table extents match mhf-unicode/resources/layout.json for this ZZ DAT.
        // Native 10A9C920 maps kinds 2/3/4/5/0 to head/body/arms/waist/legs.
        for (kind, root, count, specs, stride, class_offset) in [
            (6, 136, 17568, 124, 52, 3),
            (7, 132, 4223, 128, 60, 4),
            (2, 100, 14594, 0, 0, 0),
            (3, 104, 13462, 0, 0, 0),
            (4, 108, 13452, 0, 0, 0),
            (5, 112, 13708, 0, 0, 0),
            (0, 116, 13514, 0, 0, 0),
        ] {
            let names = get::<usize>(dat + root);
            let specs = if specs == 0 {
                0
            } else {
                get::<usize>(dat + specs)
            };
            if names == 0 {
                continue;
            }
            for id in 0..count {
                let weapon = if specs == 0 {
                    None
                } else {
                    Some(get::<u8>(specs + id * stride + class_offset))
                };
                if weapon.is_some_and(|weapon| weapon >= 14) {
                    continue;
                }
                if let Some(name) = text(get::<usize>(names + id * 4)) {
                    catalog.equipment.push(Equipment {
                        kind,
                        id: id as u16,
                        weapon,
                        name,
                    });
                }
            }
        }
        let directory = get::<usize>(dat + 389 * 4);
        for weapon in 0..14 {
            for id in [0, 6, 9] {
                catalog.actions[weapon].push(Action {
                    group: 0,
                    id,
                    weapon: weapon as u8,
                });
            }
            if directory == 0 {
                continue;
            }
            let count = get::<u32>(directory + weapon * 8);
            if count > 256 {
                continue;
            }
            // Native 10A67790 uses this exact count and a 24-byte action record.
            for id in 0..count {
                catalog.actions[weapon].push(Action {
                    group: 1,
                    id: id as u8,
                    weapon: weapon as u8,
                });
            }
        }
    }
    catalog
}

unsafe fn initialize_catalog(state: &State, runtime: &mut Runtime) {
    if !runtime.catalog_ready {
        runtime.catalog = Arc::new(unsafe { catalog(state) });
        runtime.catalog_ready = true;
        runtime.message = "本地临时猎人，正在加载任务".into();
    }
}

unsafe fn snapshot(state: &State, runtime: &Runtime) -> DebugSnapshot {
    let mut snapshot = DebugSnapshot {
        quest_id: state.quest_id,
        catalog: runtime.catalog.clone(),
        message: runtime.message.clone(),
        monster: runtime.monster.as_ref().map(|control| control.species),
        controlling_monster: state.controlled_monster.load(Ordering::Relaxed) != 0,
        monster_actions: runtime
            .monster
            .as_ref()
            .map(|control| control.actions.clone())
            .unwrap_or_default(),
        camera_distance: f32::from_bits(state.camera_distance.load(Ordering::Relaxed)),
        camera_pitch: f32::from_bits(state.camera_pitch.load(Ordering::Relaxed)),
        ..Default::default()
    };
    unsafe {
        let scene = state.read::<usize>(0x1e7fff3c);
        if scene == 0 {
            return snapshot;
        }
        snapshot.scene = get(scene);
        snapshot.area = get(scene + 20);
        snapshot.map = get(scene + 52);
        let index = usize::from(get::<u8>(scene + 9208));
        if index >= 4 {
            return snapshot;
        }
        let player = state.address(0x1dc6b750) + index * 4176;
        snapshot.ready = snapshot.scene == 2
            && get::<u8>(scene + 1) == 0
            && get::<u8>(player) != 0
            && get::<u8>(player + 16) == 0
            && get::<u8>(player + 2042) == 0
            && get::<u32>(player + 1656) != 0;
        if snapshot.ready {
            snapshot.areas = area::areas(state);
            snapshot.combat = combat::snapshot(state);
        }
        snapshot.weapon = get(player + 3);
        let actor = monster::actor(state, runtime).unwrap_or(player);
        snapshot.action_group = get(actor + 21);
        snapshot.action_id = get(actor + 20);
        snapshot.action_stage = get(actor + 5);
        snapshot.animation = get(actor + 812);
        snapshot.frame = get(actor + 476);
        snapshot.position = [get(actor + 172), get(actor + 176), get(actor + 180)];
        let save = state.read::<usize>(0x11a3ee2c);
        if save != 0 {
            for slot in 0..6 {
                snapshot.equipment.push((
                    get(save + 128441 + 16 * slot),
                    get(save + 128442 + 16 * slot),
                ));
            }
            if let Some((kind, id)) = snapshot.equipment.first()
                && let Some(item) = runtime
                    .catalog
                    .equipment
                    .iter()
                    .find(|item| (item.kind, item.id) == (*kind, *id))
            {
                snapshot.equipped_weapon = item.weapon.unwrap_or(snapshot.weapon);
            }
        }
    }
    snapshot
}

unsafe fn trigger_action(state: &State, action: Action) {
    unsafe {
        let scene = state.read::<usize>(0x1e7fff3c);
        let player = state.address(0x1dc6b750) + usize::from(get::<u8>(scene + 9208)) * 4176;
        if action.group == 1 {
            put(player + 18, 1_u8);
        }
        let change: unsafe extern "C" fn(usize, i16, i16, i16, u8) -> i16 =
            transmute(state.address(0x10a80a00));
        change(player, i16::from(action.group), i16::from(action.id), 2, 1);
    }
}

unsafe extern "C" fn dispatch() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x008fcee0) };
        return unsafe { original() };
    };
    unsafe {
        if session_started(state) {
            let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            initialize_catalog(state, &mut runtime);
            for command in state.control.commands() {
                let current = snapshot(state, &runtime);
                let result = match command {
                    DebugCommand::Exit => {
                        monster::release(state, &mut runtime);
                        state.write(0x1e866cb8, 1_i32);
                        Ok("结束调试".into())
                    }
                    DebugCommand::CameraDistance(distance) if distance.is_finite() => {
                        state
                            .camera_distance
                            .store(distance.clamp(300.0, 5000.0).to_bits(), Ordering::Relaxed);
                        Ok("已调整变身镜头距离".into())
                    }
                    DebugCommand::CameraPitch(degrees) if degrees.is_finite() => {
                        state
                            .camera_pitch
                            .store(degrees.clamp(-60.0, 80.0).to_bits(), Ordering::Relaxed);
                        Ok("已调整变身镜头垂直角度".into())
                    }
                    DebugCommand::Restart if current.ready || current.scene == 5 => {
                        runtime.pending_action = None;
                        restart(state, &mut runtime).map(|()| "正在重开任务".into())
                    }
                    DebugCommand::FollowEquipment if current.ready || current.scene == 5 => {
                        runtime.moveset = None;
                        runtime.pending_action = None;
                        restart(state, &mut runtime).map(|()| "正在恢复当前装备的招式".into())
                    }
                    DebugCommand::Equip { kind, id } if current.ready => {
                        if let Some(item) = runtime
                            .catalog
                            .equipment
                            .iter()
                            .find(|item| (item.kind, item.id) == (kind, id))
                        {
                            let name = item.name.clone();
                            equipment::equip(state, runtime.moveset, kind, id).map(|()| {
                                runtime.pending_action = None;
                                format!("已热替换为 {name}")
                            })
                        } else {
                            Err("装备编号无效".into())
                        }
                    }
                    DebugCommand::Action(action) if current.ready && runtime.monster.is_none() => {
                        if !runtime
                            .catalog
                            .actions
                            .get(action.weapon as usize)
                            .is_some_and(|actions| actions.contains(&action))
                        {
                            Err("所选武器没有此招式".into())
                        } else if action.weapon != current.weapon {
                            runtime.moveset = Some(action.weapon);
                            runtime.pending_action = Some(action);
                            runtime.ready_frames = 0;
                            restart(state, &mut runtime).map(|()| {
                                format!(
                                    "保留装备，正在载入{}招式资源",
                                    super::NATIVE_WEAPON_NAMES[action.weapon as usize]
                                )
                            })
                        } else {
                            trigger_action(state, action);
                            Ok(format!("已触发 {}", action.label()))
                        }
                    }
                    DebugCommand::Transform(species) if current.ready => {
                        monster::transform(state, &mut runtime, species, &current)
                    }
                    DebugCommand::TransformAction { species, action } if current.ready => {
                        if current.monster == Some(species) && current.controlling_monster {
                            monster::trigger(state, &runtime, action)
                        } else if runtime
                            .catalog
                            .monsters
                            .iter()
                            .find(|monster| monster.id == species)
                            .is_some_and(|monster| monster.actions.contains(&action))
                        {
                            monster::transform(state, &mut runtime, species, &current).map(
                                |message| {
                                    runtime.monster.as_mut().unwrap().pending_action = Some(action);
                                    format!("{message}，随后执行{}", action.label())
                                },
                            )
                        } else {
                            Err("所选怪物没有此招式".into())
                        }
                    }
                    DebugCommand::ChangeArea(destination) if current.ready => {
                        area::change(state, destination)
                    }
                    DebugCommand::RestoreHunter
                        if runtime.monster.is_some() && (current.ready || current.scene == 5) =>
                    {
                        monster::release(state, &mut runtime);
                        runtime.monster = None;
                        runtime.pending_action = None;
                        state
                            .session
                            .reset_quest()
                            .map_err(|error| error.to_string())
                            .and_then(|()| restart(state, &mut runtime))
                            .map(|()| "正在恢复猎人与原始任务".into())
                    }
                    DebugCommand::MonsterAction(action) if current.controlling_monster => {
                        monster::trigger(state, &runtime, action)
                    }
                    DebugCommand::NextMonsterAction if current.controlling_monster => {
                        monster::next_action(state, &runtime)
                    }
                    _ => Err("等待猎人进入任务后再操作".into()),
                };
                runtime.message = result.unwrap_or_else(|error| error);
            }
            monster::before_frame(state, &mut runtime);
        }
        let original: unsafe extern "C" fn() -> i32 = transmute(state.dispatch);
        let result = original();
        if session_started(state) {
            let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            initialize_catalog(state, &mut runtime);
            monster::after_frame(state, &mut runtime);
            if let Some(action) = runtime.pending_action {
                let current = snapshot(state, &runtime);
                if current.ready && current.weapon == action.weapon {
                    runtime.ready_frames = runtime.ready_frames.saturating_add(1);
                    if runtime.ready_frames >= 2 {
                        trigger_action(state, action);
                        runtime.pending_action = None;
                        runtime.message = format!(
                            "装备保持不变，已触发{}的{}",
                            super::NATIVE_WEAPON_NAMES[action.weapon as usize],
                            action.label()
                        );
                    }
                } else {
                    runtime.ready_frames = 0;
                }
            }
            state.control.publish(snapshot(state, &runtime));
        }
        result
    }
}

fn session_started(state: &State) -> bool {
    state.session.snapshot().hunter_initialized
}

#[cfg(test)]
mod text_tests {
    use super::decode_catalog_text;

    #[test]
    fn catalog_preserves_ascii_names() {
        assert_eq!(decode_catalog_text(b"Iron Sword").unwrap(), "Iron Sword");
    }

    #[test]
    fn catalog_decodes_original_cp932() {
        assert_eq!(
            decode_catalog_text(b"\x83\x65\x83\x58\x83\x67").unwrap(),
            "テスト"
        );
        assert!(decode_catalog_text(b"\x83").is_none());
    }
}

#[cfg(test)]
mod hook_tests {
    use super::{SIGNATURES, validate};

    #[test]
    fn debug_validation_does_not_revalidate_the_already_hooked_offline_bootstrap() {
        let size = SIGNATURES
            .iter()
            .map(|(rva, bytes)| rva + bytes.len())
            .max()
            .unwrap();
        let mut image = vec![0; size];
        for &(rva, bytes) in SIGNATURES {
            image[rva..rva + bytes.len()].copy_from_slice(bytes);
        }
        image[0x008d25a0..0x008d25a8].fill(0xe9);
        image[0x00817950..0x00817958].fill(0xe9);
        assert!(unsafe { validate(image.as_ptr() as usize) }.is_ok());
        image[0x008fcee0] ^= 0xff;
        assert!(unsafe { validate(image.as_ptr() as usize) }.is_err());
    }
}

#[cfg(test)]
#[path = "native/lifecycle_tests.rs"]
mod lifecycle_tests;
