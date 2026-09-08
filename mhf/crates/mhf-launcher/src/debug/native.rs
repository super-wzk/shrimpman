mod area;
mod combat;
mod monster;

use super::{Action, Catalog, DebugCommand, DebugSession, DebugSnapshot, Equipment};
use mhf_hooks::{HookGuard, HookSlot};
use std::{
    ffi::{CStr, c_char, c_void},
    mem::transmute,
    ptr,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
};
use windows::Win32::Foundation::HMODULE;
#[cfg(not(feature = "translation"))]
use windows::Win32::Globalization::{MB_ERR_INVALID_CHARS, MultiByteToWideChar};

static SLOT: HookSlot<State> = HookSlot::new();
static BASE: AtomicUsize = AtomicUsize::new(0);

type HurtboxCopies = std::collections::HashMap<(usize, usize), Box<[[u8; 40]]>>;

pub(crate) struct State {
    base: usize,
    session: DebugSession,
    bootstrap: usize,
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
    started: AtomicBool,
    runtime: Mutex<Runtime>,
}

#[derive(Default)]
struct Runtime {
    catalog: Arc<Catalog>,
    message: String,
    moveset: Option<u8>,
    pending_action: Option<Action>,
    ready_frames: u8,
    monster: Option<monster::Control>,
    quest_override: Option<Vec<u8>>,
}

impl State {
    fn address(&self, va: usize) -> usize {
        self.base + va - 0x1000_0000
    }
    unsafe fn read<T: Copy>(&self, va: usize) -> T {
        unsafe { get(self.address(va)) }
    }
    unsafe fn write<T>(&self, va: usize, value: T) {
        unsafe { put(self.address(va), value) }
    }
    unsafe fn call(&self, va: usize) {
        let function: unsafe extern "C" fn() = unsafe { transmute(self.address(va)) };
        unsafe { function() };
    }
}

unsafe fn get<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_unaligned(address as *const T) }
}
unsafe fn put<T>(address: usize, value: T) {
    unsafe { ptr::write_unaligned(address as *mut T, value) }
}

/// Callers stop all native callbacks before uninstalling this guard or releasing the DLL.
pub(crate) unsafe fn install(
    module: HMODULE,
    session: DebugSession,
) -> Result<HookGuard<State>, String> {
    let base = module.0 as usize;
    // Supported ZZ HD executable. These are instruction boundaries, not pattern scans.
    for (rva, expected) in [
        (
            0x008d25a0,
            &[0x55, 0x8b, 0xec, 0x51, 0x53, 0x56, 0x8b, 0x75][..],
        ),
        (
            0x01501c30,
            &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x18, 0x02, 0x00][..],
        ),
        (0x00817950, &[0x55, 0x8b, 0xec, 0x51, 0x0f, 0xbe, 0x05][..]),
        (0x008fcee0, &[0x55, 0x8b, 0xec, 0x51, 0x0f, 0xb7, 0x05][..]),
        (
            0x0089e510,
            &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x1c, 0x53, 0x33][..],
        ),
        (
            0x008696d0,
            &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x83, 0xec][..],
        ),
        (0x00a5b800, &[0x55, 0x8b, 0xec, 0x51, 0xa1][..]),
        (
            0x00baee10,
            &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf0, 0x83, 0xec, 0x68][..],
        ),
        (0x00aa4470, &[0x56, 0x8b, 0x35][..]),
        (0x00b65fd0, &[0x8a, 0x48, 0x03, 0x80, 0xe1, 0x03][..]),
        (0x00b4b9f0, &[0x53, 0x8b, 0x1d][..]),
        (
            0x00a94d25,
            &[0x66, 0x83, 0x78, 0x08, 0x00, 0x53, 0x8b, 0x58, 0x6c, 0x57][..],
        ),
        (0x00aaa420, &[0x55, 0x8b, 0xec, 0x53, 0x56, 0x57][..]),
        (0x00b7b570, &[0x56, 0xe8, 0x5a, 0xf7, 0xff, 0xff][..]),
        (0x008b6bf0, &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x24][..]),
        (0x008b7b60, &[0x55, 0x8b, 0xec, 0x80, 0x3e, 0x00][..]),
        (0x00846ca0, &[0x55, 0x8b, 0xec, 0x56, 0x8b, 0xf0][..]),
        (0x00aa0d70, &[0x0f, 0xb7, 0x8a, 0x24, 0x06, 0x00, 0x00][..]),
    ] {
        if unsafe { std::slice::from_raw_parts((base + rva) as *const u8, expected.len()) }
            != expected
        {
            return Err(format!("不支持此游戏 DLL 的调试接口：RVA {rva:#x}"));
        }
    }
    BASE.store(base, Ordering::Relaxed);
    let mut hooks = SLOT.prepare()?;
    let bootstrap = unsafe {
        hooks.create(
            "debug bootstrap",
            (base + 0x008d25a0) as _,
            bootstrap as *mut c_void,
        )
    }?;
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
    unsafe {
        hooks.create(
            "local quest request",
            (base + 0x01501c30) as _,
            request_quest as *mut c_void,
        )?;
        hooks.create(
            "local quest delivery",
            (base + 0x00817950) as _,
            poll_quest as *mut c_void,
        )?;
        hooks.create_api(c"ws2_32.dll", c"connect", reject_connection as *mut c_void)?;
        hooks.create_api(
            c"ws2_32.dll",
            c"WSAConnect",
            reject_wsa_connection as *mut c_void,
        )?;
        hooks.install(State {
            base,
            session,
            bootstrap: bootstrap as usize,
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
            started: AtomicBool::new(false),
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

#[link(name = "ws2_32")]
unsafe extern "system" {
    fn WSASetLastError(error: i32);
}

unsafe extern "system" fn reject_connection(_: usize, _: *const c_void, _: i32) -> i32 {
    unsafe { WSASetLastError(10013) }; // WSAEACCES: this process is an offline session.
    -1
}
unsafe extern "system" fn reject_wsa_connection(
    socket: usize,
    name: *const c_void,
    len: i32,
    _: *const c_void,
    _: *mut c_void,
    _: *const c_void,
    _: *const c_void,
) -> i32 {
    unsafe { reject_connection(socket, name, len) }
}

unsafe extern "thiscall" fn request_quest(this: *mut c_void, kind: u8, name: *const c_char) -> u8 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "thiscall" fn(*mut c_void, u8, *const c_char) -> u8 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x01501c30) };
        return unsafe { original(this, kind, name) };
    };
    let requested = unsafe { CStr::from_ptr(name) }.to_bytes();
    let expected = format!("{:05}", state.session.quest.id);
    if kind != 0 || !requested.starts_with(expected.as_bytes()) {
        return 0;
    }
    unsafe { state.write(0x1e4528a4, 1_u32) };
    1
}

unsafe extern "C" fn poll_quest() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x00817950) };
        return unsafe { original() };
    };
    unsafe {
        if state.read::<u8>(0x1e76afea) == 1 {
            let destination = state.read::<usize>(0x1e774b78) as *mut u8;
            if destination.is_null() {
                return 0;
            }
            let runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            let bytes = runtime
                .quest_override
                .as_deref()
                .unwrap_or(&state.session.quest.bytes);
            ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
            state.write(0x1e774b74, bytes.len() as u32);
            state.write(0x1e4528a4, 0_u32);
            state.write(0x1e76afea, 2_u8);
            return 0;
        }
        i32::from(state.read::<u8>(0x1e76afea))
    }
}

unsafe extern "C" fn bootstrap(task: *mut u8) -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(*mut u8) -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x008d25a0) };
        return unsafe { original(task) };
    };
    unsafe {
        if *task.add(8) == 0 {
            let original: unsafe extern "C" fn(*mut u8) -> i32 = transmute(state.bootstrap);
            return original(task);
        }
        if !state.started.swap(true, Ordering::Relaxed) {
            let appearance: unsafe extern "C" fn(i16) = transmute(state.address(0x10834940));
            let create: unsafe extern "C" fn(i8) = transmute(state.address(0x10834bf0));
            appearance(0);
            create(-1);
            let save = state.read::<usize>(0x11a3ee2c);
            ptr::copy_nonoverlapping(c"Debug".as_ptr().cast(), (save + 88) as *mut u8, 6);
            let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            runtime.catalog = Arc::new(catalog(state));
            prepare_scene(state);
            runtime.message = "本地临时猎人，正在加载任务".into();
        }
        ptr::write_unaligned(task.cast::<u16>(), 0);
    }
    0
}

unsafe fn prepare_scene(state: &State) {
    unsafe {
        state.call(0x1089def0);
        state.call(0x107a1540);
        let scene = state.read::<usize>(0x1e7fff3c);
        put(scene + 9208, 0_u8);
        put(scene + 9210, 1_u8);
        put(scene + 9215, 0_u8);
        put(scene + 9672, 1_u8);
        state.write(0x1ed52870, 0_u8);
        state.write(0x1ed52951, 0_u16);
        state.write(0x1ed6bca0, 9_u8);
        state.write(0x1ed7d418, state.session.quest.id);
        state.write(0x1ed5291c, 0_u16);
        state.call(0x10a7fcf0); // Populate the party equipment from the current save.
        let record = state.address(0x1ee3df00) as *mut u8;
        ptr::write_bytes(record, 0, 0x390);
        put(record as usize, 0x30_u32);
        ptr::copy_nonoverlapping(
            state
                .session
                .quest
                .bytes
                .as_ptr()
                .add(state.session.quest.properties),
            record.add(16),
            320,
        );
        state.write(0x1e76af08, 1_u16);
        let task = state.address(0x1edb98c0) as *mut u8;
        ptr::write_bytes(task, 0, 32);
        put(task as usize, 12_u32);
        put(task as usize + 4, state.address(0x10899d90));
    }
}

unsafe fn restart(state: &State, runtime: &mut Runtime) {
    unsafe {
        monster::release(state, runtime);
        *state.combat.lock().unwrap_or_else(PoisonError::into_inner) = Default::default();
        if let Some(control) = &mut runtime.monster {
            control.waiting = true;
            control.resume_at_arrival = false;
            control.instance = None;
        }
        state.call(0x10b738a0);
        ptr::write_bytes(state.address(0x1edb9820) as *mut u8, 0, 0x120);
        prepare_scene(state);
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
    #[cfg(feature = "translation")]
    {
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
    #[cfg(not(feature = "translation"))]
    {
        // Without language hooks, the supported Japanese DAT keeps its CP932 text.
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
        // Name-table extents match translations/resources.json for this ZZ DAT.
        for (kind, root, count, specs, stride, class_offset) in [
            (6, 136, 17568, 124, 52, 3),
            (7, 132, 4223, 128, 60, 4),
            (0, 100, 14594, 0, 0, 0),
            (2, 104, 13462, 0, 0, 0),
            (3, 108, 13452, 0, 0, 0),
            (4, 112, 13708, 0, 0, 0),
            (5, 116, 13514, 0, 0, 0),
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

unsafe fn equipment(state: &State, kind: u8, id: u16) -> Result<(), String> {
    unsafe {
        let save = state.read::<usize>(0x11a3ee2c);
        let add: unsafe extern "C" fn(usize, u8, u16, u16) -> i16 =
            transmute(state.address(0x10ba6b10));
        let capacity: unsafe extern "C" fn(usize) -> i16 = transmute(state.address(0x10ba9ad0));
        let count = i32::from(capacity(save)) * 100;
        if !(1..=8000).contains(&count) {
            return Err("装备箱状态无效".into());
        }
        let existing = (0..count as usize).find(|index| {
            let item = save + 212 + index * 16;
            get::<u8>(item) & 1 != 0 && get::<u8>(item + 1) == kind && get::<u16>(item + 2) == id
        });
        let index = existing.map_or_else(|| add(save, kind, id, 0), |index| index as i16);
        if index < 0 {
            return Err("临时装备箱已满".into());
        }
        // Equip uses AX=inventory index, ECX=save; rebuild uses EAX=save.
        std::arch::asm!("call edx", in("edx") state.address(0x10ba7160),
            inlateout("eax") index as u32 => _, in("ecx") save, clobber_abi("C"));
        std::arch::asm!("call edx", in("edx") state.address(0x10ba7eb0),
            inlateout("eax") save => _, clobber_abi("C"));
    }
    Ok(())
}

unsafe fn snapshot(state: &State, runtime: &Runtime) -> DebugSnapshot {
    let mut snapshot = DebugSnapshot {
        quest_id: state.session.quest.id,
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
        if state.started.load(Ordering::Relaxed) {
            let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            for command in state.session.control.commands() {
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
                        restart(state, &mut runtime);
                        Ok("正在重开任务".into())
                    }
                    DebugCommand::FollowEquipment if current.ready || current.scene == 5 => {
                        runtime.moveset = None;
                        runtime.pending_action = None;
                        restart(state, &mut runtime);
                        Ok("正在恢复当前装备的招式".into())
                    }
                    DebugCommand::Equip { kind, id } if current.ready => {
                        if let Some(item) = runtime
                            .catalog
                            .equipment
                            .iter()
                            .find(|item| (item.kind, item.id) == (kind, id))
                        {
                            let name = item.name.clone();
                            equipment(state, kind, id).map(|()| {
                                runtime.pending_action = None;
                                restart(state, &mut runtime);
                                format!("已选择 {name}，正在重新加载")
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
                            restart(state, &mut runtime);
                            Ok(format!(
                                "保留装备，正在载入{}招式资源",
                                super::NATIVE_WEAPON_NAMES[action.weapon as usize]
                            ))
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
                        area::change(state, &runtime, destination)
                    }
                    DebugCommand::RestoreHunter
                        if runtime.monster.is_some() && (current.ready || current.scene == 5) =>
                    {
                        monster::release(state, &mut runtime);
                        runtime.monster = None;
                        runtime.quest_override = None;
                        runtime.pending_action = None;
                        restart(state, &mut runtime);
                        Ok("正在恢复猎人与原始任务".into())
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
        if state.started.load(Ordering::Relaxed) {
            let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
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
            state.session.control.publish(snapshot(state, &runtime));
        }
        result
    }
}

#[cfg(test)]
mod text_tests {
    use super::decode_catalog_text;

    #[test]
    fn catalog_preserves_ascii_names() {
        assert_eq!(decode_catalog_text(b"Iron Sword").unwrap(), "Iron Sword");
    }

    #[cfg(not(feature = "translation"))]
    #[test]
    fn catalog_decodes_original_cp932_without_translation() {
        assert_eq!(
            decode_catalog_text(b"\x83\x65\x83\x58\x83\x67").unwrap(),
            "テスト"
        );
        assert!(decode_catalog_text(b"\x83").is_none());
    }

    #[cfg(feature = "translation")]
    #[test]
    fn catalog_decodes_utf8_with_translation() {
        assert_eq!(
            decode_catalog_text("铁剑・テスト".as_bytes()).unwrap(),
            "铁剑・テスト"
        );
    }
}
