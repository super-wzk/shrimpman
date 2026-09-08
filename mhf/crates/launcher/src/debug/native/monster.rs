use super::{BASE, Runtime, SLOT, State, get, put, restart};
use crate::debug::{DebugSnapshot, MonsterAction, MonsterInput};
use std::{mem::transmute, sync::atomic::Ordering, time::Instant};

pub(super) const POOL: usize = 0x1ed7ad2c;
pub(super) const STRIDE: usize = 3824;
pub(super) const SLOTS: usize = 40;

pub(super) struct Control {
    pub(super) species: u8,
    pub(super) waiting: bool,
    pub(super) actions: Vec<MonsterAction>,
    pub(super) pending_action: Option<MonsterAction>,
    pub(super) resume_at_arrival: bool,
    pub(super) instance: Option<(u8, u32)>,
    spawn_offset: usize,
    area: u16,
    position: [f32; 3],
    yaw: u16,
    pool: usize,
    actor: usize,
    model: usize,
    hunter: usize,
    visible: u8,
    wait_frames: u16,
    tick: Instant,
}

pub(super) unsafe fn hunter(state: &State) -> Option<usize> {
    unsafe {
        let scene = state.read::<usize>(0x1e7fff3c);
        if scene == 0 {
            return None;
        }
        let index = usize::from(get::<u8>(scene + 9208));
        (index < 4).then(|| state.address(0x1dc6b750) + index * 4176)
    }
}

pub(super) unsafe fn actor(state: &State, runtime: &Runtime) -> Option<usize> {
    let control = runtime.monster.as_ref()?;
    unsafe {
        let pool = state.read::<usize>(POOL);
        if pool == 0 || pool != control.pool || control.actor == 0 {
            return None;
        }
        let offset = control.actor.checked_sub(pool)?;
        if offset >= STRIDE * SLOTS || offset % STRIDE != 0 {
            return None;
        }
        let actor = control.actor;
        (get::<u8>(actor) != 0
            && control
                .instance
                .is_some_and(|(_, serial)| get::<u32>(actor + 3448) == serial)
            && get::<u8>(actor + 3) == control.species
            && get::<u8>(actor + 4) == 1
            && get::<u8>(actor + 16) == 1
            && get::<u8>(actor + 30) == 0
            && get::<usize>(actor + 1656) == control.model
            && control.model != 0)
            .then_some(actor)
    }
}

pub(super) unsafe fn release(state: &State, runtime: &mut Runtime) {
    state.controlled_monster.store(0, Ordering::Relaxed);
    if let Some(control) = &mut runtime.monster {
        // Hunter storage is static; do not touch the monster pool during teardown.
        if control.hunter != 0 {
            unsafe { put(control.hunter + 1, control.visible) };
        }
        control.actor = 0;
        control.model = 0;
        control.pool = 0;
        control.hunter = 0;
        control.wait_frames = 0;
        control.pending_action = None;
    }
    state.control.set_monster_input(MonsterInput::default());
}

pub(super) unsafe fn transform(
    state: &State,
    runtime: &mut Runtime,
    species: u8,
    current: &DebugSnapshot,
) -> Result<String, String> {
    let selected = runtime
        .catalog
        .monsters
        .iter()
        .find(|monster| monster.id == species)
        .ok_or("客户端没有此怪物种类")?;
    let name = selected.name;
    let actions = selected.actions.clone();
    unsafe {
        let source = actor(state, runtime)
            .or_else(|| hunter(state))
            .ok_or("猎人尚未初始化")?;
        let yaw = get::<u16>(source + 164);
        let spawn_offset =
            state
                .session
                .prepare_monster(species, current.area, current.position, yaw)?;
        release(state, runtime);
        runtime.monster = Some(Control {
            species,
            waiting: true,
            actions,
            pending_action: None,
            resume_at_arrival: false,
            instance: None,
            spawn_offset,
            area: current.area,
            position: current.position,
            yaw,
            pool: 0,
            actor: 0,
            model: 0,
            hunter: 0,
            visible: 1,
            wait_frames: 0,
            tick: Instant::now(),
        });
        runtime.pending_action = None;
        restart(state, runtime)?;
    }
    Ok(format!("正在载入{name}，重载后自动变身"))
}

pub(super) unsafe fn active_scene(state: &State) -> bool {
    unsafe {
        let scene = state.read::<usize>(0x1e7fff3c);
        scene != 0 && get::<u8>(scene) == 2 && get::<u8>(scene + 1) == 0
    }
}

/// Keep the species and its loaded resources across the native area loader. The
/// exit detector (10B4BC00 -> 10B4B9F0) supplies hunter +2046/+2048/+1730;
/// 10A6D0F0 installs the destination and clears +2042 when loading is finished.
unsafe fn area_transition(state: &State, runtime: &mut Runtime) -> bool {
    unsafe {
        let Some(control) = runtime.monster.as_ref() else {
            return false;
        };
        let Some(player) = hunter(state) else {
            return false;
        };
        let scene = state.read::<usize>(0x1e7fff3c);
        if get::<u8>(scene) != 2 {
            return false;
        }
        let transitioning = get::<u8>(player + 2042) != 0;
        if control.resume_at_arrival {
            return transitioning
                || !active_scene(state)
                || get::<u16>(player + 2040) != get::<u16>(scene + 20);
        }
        if !transitioning || state.controlled_monster.load(Ordering::Relaxed) == 0 {
            return false;
        }
        let area = get::<u16>(player + 2046);
        let position = [
            get::<f32>(player + 2048),
            get::<f32>(player + 2052),
            get::<f32>(player + 2056),
        ];
        if area == u16::MAX || !position.iter().all(|value| value.is_finite()) {
            return false;
        }
        if let Some(target) = actor(state, runtime) {
            // Primary monsters survive an ordinary area change. Move this actor
            // to the destination before native visibility and unload decisions.
            put(target + 2040, area);
            for (index, coordinate) in position.into_iter().enumerate() {
                put(target + 172 + index * 4, coordinate);
            }
            put(target + 164, u32::from(get::<u16>(player + 1730)));
        }
        release(state, runtime);
        let control = runtime.monster.as_mut().unwrap();
        control.waiting = true;
        control.resume_at_arrival = true;
        runtime.message = format!("正在前往区域 {area}，载入后继续操控怪物");
        true
    }
}

pub(super) unsafe fn before_frame(state: &State, runtime: &mut Runtime) {
    unsafe {
        if area_transition(state, runtime) {
            return;
        }
        let Some(target) = actor(state, runtime) else {
            if state.controlled_monster.load(Ordering::Relaxed) != 0 {
                release(state, runtime);
                runtime.message = "怪物已卸载，已停止操控；可重新变身或恢复猎人".into();
            }
            return;
        };
        if !active_scene(state) {
            release(state, runtime);
            return;
        }
        let input = state.control.monster_input();
        let control = runtime.monster.as_mut().unwrap();
        let now = Instant::now();
        let delta = now.duration_since(control.tick).as_secs_f32().min(0.05);
        control.tick = now;
        let radians = f32::from(get::<u16>(target + 164)) * std::f32::consts::TAU / 65536.0;
        let forward = camera_forward(state).unwrap_or([radians.sin(), radians.cos()]);
        // LookAtRH uses +Y up: right = forward cross up = (-forward.z, forward.x).
        // Project onto the ground so camera pitch does not change walking speed.
        let mut direction = [
            forward[0] * input.forward - forward[1] * input.sideways,
            forward[1] * input.forward + forward[0] * input.sideways,
        ];
        let length = direction[0].hypot(direction[1]).max(1.0);
        direction[0] /= length;
        direction[1] /= length;
        if direction[0] != 0.0 || direction[1] != 0.0 {
            let yaw = (direction[0].atan2(direction[1]) * 65536.0 / std::f32::consts::TAU).round()
                as i32 as u16;
            put(target + 164, u32::from(yaw));
        }
        let distance = input.speed.clamp(0.0, 2400.0) * delta;
        let position = [get(target + 172), get(target + 176), get(target + 180)];
        if super::area::enter_at_position(
            state,
            get(target + 2040),
            position,
            [direction[0] * distance, direction[1] * distance],
        ) {
            area_transition(state, runtime);
            return;
        }
        put(
            target + 172,
            get::<f32>(target + 172) + direction[0] * distance,
        );
        put(
            target + 180,
            get::<f32>(target + 180) + direction[1] * distance,
        );
        put(
            target + 176,
            get::<f32>(target + 176)
                + input.vertical.clamp(-1.0, 1.0) * input.speed.clamp(0.0, 2400.0) * delta,
        );
        // Native monster movement/collision and attack processing still run below.
        sync_hunter(state, runtime.monster.as_ref().unwrap());
    }
}

unsafe fn camera_forward(state: &State) -> Option<[f32; 2]> {
    unsafe {
        let camera = state.read::<usize>(0x1edaad60);
        if camera == 0 {
            return None;
        }
        let x = get::<f32>(camera + 140) - get::<f32>(camera + 128);
        let z = get::<f32>(camera + 148) - get::<f32>(camera + 136);
        let length = x.hypot(z);
        (length.is_finite() && length > 0.001).then(|| [x / length, z / length])
    }
}

unsafe fn sync_hunter(state: &State, control: &Control) {
    unsafe {
        for offset in [172, 176, 180] {
            put(control.hunter + offset, get::<f32>(control.actor + offset));
        }
        put(control.hunter + 164, get::<u32>(control.actor + 164));
        put(control.hunter + 2040, get::<u16>(control.actor + 2040));
        // The native map draws +2068/+2072/+2076, not world XYZ. Suspending
        // hunter updates also suspended 10A94D20, so refresh this projection
        // explicitly after copying the controlled monster's area and position.
        let quest = state.read::<usize>(0x1e8001ec);
        if quest != 0 && get::<usize>(quest + 108) != 0 {
            std::arch::asm!(
                "push esi",
                "mov esi, eax",
                "call edi",
                "pop esi",
                in("edi") state.address(0x10a94d20),
                inlateout("eax") control.hunter => _,
                clobber_abi("C"),
            );
        }
        // AI needs the hunter proxy to remain present. Only the render callback
        // hides its model; enemy hits against this proxy are routed to the monster.
        put(control.hunter + 1, 1_u8);
    }
}

pub(super) unsafe fn after_frame(state: &State, runtime: &mut Runtime) {
    unsafe {
        if area_transition(state, runtime) {
            return;
        }
        if !active_scene(state) {
            if state.controlled_monster.load(Ordering::Relaxed) != 0 {
                release(state, runtime);
            }
            return;
        }
        if runtime
            .monster
            .as_ref()
            .is_some_and(|control| control.waiting)
        {
            let pool = state.read::<usize>(POOL);
            let player = hunter(state).filter(|player| {
                get::<u8>(*player) != 0
                    && get::<u8>(*player + 2042) == 0
                    && get::<usize>(*player + 1656) != 0
            });
            let control = runtime.monster.as_mut().unwrap();
            if control.instance.is_none() && pool != 0 && player.is_some() {
                let scene = state.read::<usize>(0x1e7fff3c);
                let resource =
                    (0..6).find(|index| get::<u8>(scene + 42 + index) == control.species);
                let buffer = state.read::<usize>(0x1ed528f4);
                if let Some(resource) = resource
                    && buffer != 0
                    && state.session.override_contains(control.spawn_offset, 60)
                {
                    let create: unsafe extern "C" fn(usize, usize, u8) -> usize =
                        transmute(state.address(0x10aaa420));
                    let created = create(buffer + control.spawn_offset, 0, resource as u8);
                    if let Some(offset) = created.checked_sub(pool)
                        && offset < STRIDE * SLOTS
                        && offset % STRIDE == 0
                    {
                        control.instance = Some(((offset / STRIDE) as u8, get(created + 3448)));
                    }
                }
            }
            // Slot and allocation serial identify our actor even when the quest
            // target has the same species. Never take over a matching original.
            let target = control.instance.and_then(|(slot, serial)| {
                let target = pool + usize::from(slot) * STRIDE;
                (pool != 0
                    && get::<u8>(target) != 0
                    && get::<u32>(target + 3448) == serial
                    && get::<u8>(target + 3) == control.species
                    && get::<u8>(target + 4) == 1
                    && get::<u8>(target + 16) == 1
                    && get::<u8>(target + 30) == 0
                    && get::<usize>(target + 1656) != 0)
                    .then_some(target)
            });
            if let (Some(target), Some(player)) = (target, player) {
                let (area, position, yaw) = if control.resume_at_arrival {
                    (
                        get::<u16>(player + 2040),
                        [get(player + 172), get(player + 176), get(player + 180)],
                        get::<u16>(player + 164),
                    )
                } else {
                    (control.area, control.position, control.yaw)
                };
                control.resume_at_arrival = false;
                control.waiting = false;
                control.pool = pool;
                control.actor = target;
                control.model = get(target + 1656);
                control.hunter = player;
                control.visible = get(player + 1);
                control.tick = Instant::now();
                put(target + 2040, area);
                for (index, coordinate) in position.into_iter().enumerate() {
                    put(target + 172 + index * 4, coordinate);
                }
                put(target + 164, u32::from(yaw));
                state.controlled_monster.store(target, Ordering::Relaxed);
                runtime.message = format!(
                    "已变身为{} · 点击游戏区域即可操控",
                    super::super::monsters::NAMES[control.species as usize]
                );
            } else {
                control.wait_frames = control.wait_frames.saturating_add(1);
                if control.wait_frames >= 180 {
                    control.waiting = false;
                    runtime.message =
                        "所选怪物未完成初始化；可能需要对应专用地图，可恢复猎人或选择其他种类"
                            .into();
                }
            }
        }
        if actor(state, runtime).is_some() {
            let control = runtime.monster.as_mut().unwrap();
            let action = MonsterAction {
                group: get(control.actor + 21),
                id: get(control.actor + 20),
            };
            if action.group < 4 && !control.actions.contains(&action) {
                control.actions.push(action);
                control
                    .actions
                    .sort_by_key(|action| (action.group, action.id));
            }
            sync_hunter(state, control);
        }
        let pending = runtime.monster.as_mut().and_then(|control| {
            if control.actor != 0 {
                control.pending_action.take()
            } else {
                None
            }
        });
        if let Some(action) = pending {
            runtime.message = trigger(state, runtime, action).unwrap_or_else(|error| error);
        }
    }
}

/// Set distance and pitch before 10BAEE10 builds the view matrix. Preserve the
/// native horizontal direction, target and FOV; positive pitch places the eye above
/// the target. Reconstructing the offset avoids accumulating pitch on repeated calls.
pub(super) unsafe extern "C" fn update_camera() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x00baee10) };
        return unsafe { original() };
    };
    unsafe {
        if state.controlled_monster.load(Ordering::Relaxed) != 0 && active_scene(state) {
            let camera = state.read::<usize>(0x1edaad60);
            if camera != 0 {
                let eye: [f32; 3] = [get(camera + 128), get(camera + 132), get(camera + 136)];
                let target: [f32; 3] = [get(camera + 140), get(camera + 144), get(camera + 148)];
                let offset = [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]];
                let distance = offset.iter().map(|value| value * value).sum::<f32>().sqrt();
                let horizontal = offset[0].hypot(offset[2]);
                let requested = f32::from_bits(state.camera_distance.load(Ordering::Relaxed));
                let pitch = f32::from_bits(state.camera_pitch.load(Ordering::Relaxed)).to_radians();
                if distance.is_finite()
                    && distance > 1.0
                    && horizontal > 0.001
                    && target.iter().all(|value| value.is_finite())
                {
                    let radius = distance.max(requested);
                    let horizontal_radius = radius * pitch.cos();
                    put(
                        camera + 128,
                        target[0] + offset[0] / horizontal * horizontal_radius,
                    );
                    put(camera + 132, target[1] + radius * pitch.sin());
                    put(
                        camera + 136,
                        target[2] + offset[2] / horizontal * horizontal_radius,
                    );
                }
            }
        }
        let original: unsafe extern "C" fn() -> i32 = transmute(state.camera_update);
        original()
    }
}

pub(super) unsafe fn trigger(
    state: &State,
    runtime: &Runtime,
    action: MonsterAction,
) -> Result<String, String> {
    let target = unsafe { actor(state, runtime) }.ok_or("怪物尚未完成初始化")?;
    if !runtime.monster.as_ref().unwrap().actions.contains(&action) {
        return Err("尚未收录此怪物招式；可通过原生选招记录动作".into());
    }
    // 10850CA0 performs species-specific cleanup, then dispatches through
    // 118C38F0 (or the object's vtable). ESI=actor, stack=group,id,flags.
    // These calls preserve the native monster's attack state and hitboxes.
    unsafe {
        std::arch::asm!(
            "push esi",
            "mov esi, eax",
            "push 1",
            "push edx",
            "push ecx",
            "call edi",
            "add esp, 12",
            "pop esi",
            in("edi") state.address(0x10850ca0),
            inlateout("eax") target => _,
            in("ecx") u32::from(action.group),
            in("edx") u32::from(action.id),
            clobber_abi("C"),
        );
    }
    Ok(format!("已触发{}", action.label()))
}

pub(super) unsafe fn next_action(state: &State, runtime: &Runtime) -> Result<String, String> {
    let target = unsafe { actor(state, runtime) }.ok_or("怪物尚未完成初始化")?;
    if unsafe { get::<usize>(target + 2544) == 0 || get::<usize>(target + 2548) == 0 } {
        return Err("当前形态没有可用的原生选招脚本".into());
    }
    let select: unsafe extern "thiscall" fn(usize) -> i8 = unsafe { transmute(state.monster_ai) };
    unsafe { select(target) };
    Ok("已执行一次原生选招；实际动作会加入下方列表".into())
}

pub(super) unsafe extern "thiscall" fn select_action(target: usize) -> i8 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "thiscall" fn(usize) -> i8 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x008696d0) };
        return unsafe { original(target) };
    };
    if target == state.controlled_monster.load(Ordering::Relaxed) && target != 0 {
        return 0;
    }
    let original: unsafe extern "thiscall" fn(usize) -> i8 = unsafe { transmute(state.monster_ai) };
    unsafe { original(target) }
}

pub(super) unsafe extern "C" fn update_hunters() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x00a5b800) };
        return unsafe { original() };
    };
    // This debugger constructs one local hunter. Keep its slot as the camera's
    // anchor while the monster owns input; the original update resumes on release.
    if state.controlled_monster.load(Ordering::Relaxed) != 0 {
        return 0;
    }
    let original: unsafe extern "C" fn() -> i32 = unsafe { transmute(state.hunter_update) };
    unsafe { original() }
}
