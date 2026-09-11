//! Resource preview inside the native graphics loop, without a quest or hunter.

mod animation;
mod asset;
mod skeleton;
mod textures;
mod viewport;

use crate::preview::{AssetBundle, Bone, Camera, Command, Control, LoadedModel, Snapshot};
use mhf_hooks::{HookGuard, HookSlot, ModuleReference};
use std::{
    ffi::c_void,
    mem::transmute,
    ptr,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use windows::{
    Win32::{Foundation::HMODULE, Graphics::Direct3D9::IDirect3DDevice9},
    core::Interface,
};

static SLOT: HookSlot<State> = HookSlot::new();
static BASE: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy)]
struct Client {
    base: usize,
}
impl Client {
    fn address(self, va: usize) -> usize {
        self.base + va - 0x1000_0000
    }
    unsafe fn read<T: Copy>(self, va: usize) -> T {
        unsafe { get(self.address(va)) }
    }
}

pub(crate) struct State {
    module: ModuleReference,
    client: Client,
    control: Arc<Control>,
    bootstrap: usize,
    dispatch: usize,
    runtime: Mutex<Runtime>,
}

#[derive(Default)]
struct Runtime {
    initialized: bool,
    rendered: bool,
    viewport_reported: bool,
    snapshot: Snapshot,
    focus_bone: Option<usize>,
    center: [f32; 3],
    models: Vec<Model>,
    active_model: Option<u64>,
    next_model_id: u64,
    scene: Option<asset::NativeAsset>,
    last_frame: Option<Instant>,
}

struct Model {
    id: u64,
    bundle: AssetBundle,
    asset: Option<asset::NativeAsset>,
    visible: bool,
    error: Option<Arc<str>>,
    motion: Option<animation::NativeMotion>,
    frame: f32,
    playing: bool,
    bones: Arc<Vec<Bone>>,
}

impl Model {
    unsafe fn unload_motion(&mut self, client: Client) -> Result<(), String> {
        if let Some(motion) = &mut self.motion {
            unsafe {
                motion.release(client)?;
            }
        }
        self.motion = None;
        self.frame = 0.0;
        Ok(())
    }
    unsafe fn release(&mut self, client: Client) -> Result<(), String> {
        unsafe {
            self.unload_motion(client)?;
        }
        if let Some(asset) = &mut self.asset {
            unsafe {
                asset.release(client)?;
            }
        }
        self.asset = None;
        self.bones = Arc::new(Vec::new());
        Ok(())
    }
}

impl State {
    pub unsafe fn prepare_release(&mut self) -> Result<(), String> {
        unsafe { self.module.release() }
    }
}

unsafe fn get<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_unaligned(address as *const T) }
}
unsafe fn put<T>(address: usize, value: T) {
    unsafe { ptr::write_unaligned(address as *mut T, value) }
}

const SIGNATURES: &[(usize, &[u8])] = &[
    (
        0x10006b10,
        &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x83, 0xec, 0x40],
    ),
    (0x108fcee0, &[0x55, 0x8b, 0xec, 0x51, 0x0f, 0xb7, 0x05]),
    (
        0x108d25a0,
        &[0x55, 0x8b, 0xec, 0x51, 0x53, 0x56, 0x8b, 0x75, 0x08, 0x8a],
    ),
    (
        0x10b75370,
        &[0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d, 0x08, 0x56],
    ),
];

pub(crate) unsafe fn install(
    module: HMODULE,
    control: Arc<Control>,
) -> Result<HookGuard<State>, String> {
    let base = module.0 as usize;
    let client = Client { base };
    unsafe {
        asset::validate(client)?;
        animation::validate(client)?;
    }
    for &(address, expected) in SIGNATURES {
        if unsafe {
            std::slice::from_raw_parts(client.address(address) as *const u8, expected.len())
        } != expected
        {
            return Err(format!("不支持此游戏 DLL 的工作台接口：{address:#x}"));
        }
    }
    let retained = unsafe { ModuleReference::acquire(module) }?;
    BASE.store(base, Ordering::Relaxed);
    let mut hooks = SLOT.prepare()?;
    let bootstrap = unsafe {
        hooks.create(
            "workbench empty startup",
            client.address(0x108d25a0) as _,
            bootstrap as *mut c_void,
        )
    }?;
    let dispatch = unsafe {
        hooks.create(
            "workbench commands",
            client.address(0x108fcee0) as _,
            dispatch as *mut c_void,
        )
    }?;
    unsafe {
        hooks.install(State {
            module: retained,
            client,
            control,
            bootstrap: bootstrap as usize,
            dispatch: dispatch as usize,
            runtime: Mutex::new(Runtime::default()),
        })
    }
}

unsafe fn ready(state: &State, runtime: &Runtime) -> bool {
    runtime.initialized
        && [0x1ed528d0, 0x1ed528c4, 0x1ed528c8, 0x1ed528cc]
            .into_iter()
            .all(|address| unsafe { state.client.read::<usize>(address) != 0 })
}

unsafe fn clear_models(client: Client, runtime: &mut Runtime) -> Result<(), String> {
    // Keep every source alive until all native release calls have completed.
    for model in &mut runtime.models {
        unsafe {
            model.release(client)?;
        }
    }
    runtime.models.clear();
    runtime.active_model = None;
    runtime.focus_bone = None;
    runtime.last_frame = None;
    refresh_snapshot(runtime);
    Ok(())
}

fn active_model(runtime: &mut Runtime) -> Result<&mut Model, String> {
    let id = runtime.active_model.ok_or("请先选择预览中的模型")?;
    runtime
        .models
        .iter_mut()
        .find(|model| model.id == id)
        .ok_or_else(|| "所选模型已移除".into())
}

fn model_asset(runtime: &mut Runtime, id: u64) -> Result<&mut asset::NativeAsset, String> {
    runtime
        .models
        .iter_mut()
        .find(|model| model.id == id)
        .ok_or("模型已移除")?
        .asset
        .as_mut()
        .ok_or_else(|| "模型未加载成功，请查看该项错误或重新加入预览".into())
}

fn focus_model(runtime: &mut Runtime, id: u64) -> Result<(), String> {
    let model = runtime
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or("模型已移除")?;
    if let Some(asset) = &model.asset {
        let (center, distance) = asset.framing();
        runtime.center = center;
        runtime.snapshot.distance = distance;
    }
    runtime.active_model = Some(id);
    runtime.focus_bone = None;
    Ok(())
}

fn focus_all(runtime: &mut Runtime) -> Result<(), String> {
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    let mut found = false;
    for model in &runtime.models {
        if model.visible
            && let Some(asset) = &model.asset
        {
            let (center, distance) = asset.framing();
            let radius = distance / 2.5;
            for axis in 0..3 {
                minimum[axis] = minimum[axis].min(center[axis] - radius);
                maximum[axis] = maximum[axis].max(center[axis] + radius);
            }
            found = true;
        }
    }
    if !found {
        return Err("没有可见的模型".into());
    }
    runtime.center = std::array::from_fn(|axis| minimum[axis] * 0.5 + maximum[axis] * 0.5);
    runtime.snapshot.distance = (minimum
        .into_iter()
        .zip(maximum)
        .map(|(a, b)| (b * 0.5 - a * 0.5).powi(2))
        .sum::<f32>()
        .sqrt()
        * 2.5)
        .clamp(1.0, 100_000.0);
    runtime.focus_bone = None;
    Ok(())
}

unsafe fn add_model(client: Client, runtime: &mut Runtime, bundle: AssetBundle) -> u64 {
    if let Some(existing) = runtime
        .models
        .iter_mut()
        .find(|model| model.bundle.same_source(&bundle))
    {
        if existing.asset.is_none() || existing.error.is_some() {
            if let Err(error) = unsafe { existing.release(client) } {
                existing.error = Some(error.into());
                return existing.id;
            }
            match unsafe { asset::NativeAsset::load(client, bundle) } {
                Ok(asset) => {
                    existing.asset = Some(asset);
                    existing.error = None;
                    existing.visible = true;
                }
                Err(error) => existing.error = Some(error.into()),
            }
        } else {
            existing.visible = true;
        }
        return existing.id;
    }
    runtime.next_model_id = runtime.next_model_id.wrapping_add(1);
    let id = runtime.next_model_id;
    let result = unsafe { asset::NativeAsset::load(client, bundle.clone()) };
    let (asset, error) = match result {
        Ok(asset) => {
            eprintln!("workbench: loaded model {id}: {}", bundle.name);
            (Some(asset), None)
        }
        Err(error) => {
            eprintln!("workbench: model {id} failed: {}: {error}", bundle.name);
            (None, Some(Arc::from(error)))
        }
    };
    runtime.models.push(Model {
        id,
        bundle,
        visible: asset.is_some(),
        asset,
        error,
        motion: None,
        frame: 0.0,
        playing: true,
        bones: Arc::new(Vec::new()),
    });
    id
}

fn refresh_snapshot(runtime: &mut Runtime) {
    runtime.snapshot.models = Arc::new(
        runtime
            .models
            .iter()
            .map(|model| LoadedModel {
                id: model.id,
                name: model.bundle.name.as_str().into(),
                visible: model.visible,
                error: model.error.clone(),
                meshes: model
                    .asset
                    .as_ref()
                    .map_or_else(Arc::default, |asset| asset.meshes()),
            })
            .collect(),
    );
    runtime.snapshot.active_model = runtime.active_model;
    if let Some(model) = runtime
        .models
        .iter()
        .find(|model| Some(model.id) == runtime.active_model)
    {
        runtime.snapshot.frame = model.frame;
        runtime.snapshot.frames = model.motion.as_ref().map_or(0.0, |motion| motion.frames());
        runtime.snapshot.playing = model.playing;
        runtime.snapshot.motion = model.motion.as_ref().map(|motion| motion.name().into());
        runtime.snapshot.bone_bindings = model
            .asset
            .as_ref()
            .map_or_else(Arc::default, |asset| asset.bone_bindings());
        runtime.snapshot.bones = if model.visible {
            model.bones.clone()
        } else {
            Arc::new(Vec::new())
        };
    } else {
        runtime.snapshot.frame = 0.0;
        runtime.snapshot.frames = 0.0;
        runtime.snapshot.motion = None;
        runtime.snapshot.playing = false;
        runtime.snapshot.bones = Arc::new(Vec::new());
        runtime.snapshot.bone_bindings = Arc::default();
    }
}

unsafe fn unload_scene(client: Client, runtime: &mut Runtime) -> Result<(), String> {
    if let Some(scene) = &mut runtime.scene {
        unsafe {
            scene.release(client)?;
        }
    }
    runtime.scene = None;
    runtime.snapshot.scene = None;
    runtime.snapshot.scene_visible = false;
    Ok(())
}

unsafe fn command(
    state: &State,
    runtime: &mut Runtime,
    command: Command,
) -> Result<String, String> {
    let client = state.client;
    if matches!(command, Command::Exit) {
        unsafe {
            clear_models(client, runtime)?;
            unload_scene(client, runtime)?;
            put(client.address(0x1e866cb8), 1_i32);
        }
        return Ok("正在结束资源工作台".into());
    }
    if !unsafe { ready(state, runtime) } {
        return Err("原生资源池尚未初始化".into());
    }
    unsafe {
        match command {
            Command::LoadAssets(bundles) => {
                if bundles.is_empty() {
                    return Err("所选资源中没有可预览的模型组".into());
                }
                clear_models(client, runtime)?;
                for bundle in bundles {
                    add_model(client, runtime, bundle);
                }
                let first = runtime
                    .models
                    .iter()
                    .find(|model| model.asset.is_some())
                    .or_else(|| runtime.models.first())
                    .map(|model| model.id);
                if let Some(id) = first {
                    focus_model(runtime, id)?;
                }
                let loaded = runtime
                    .models
                    .iter()
                    .filter(|model| model.asset.is_some())
                    .count();
                Ok(format!(
                    "已载入 {loaded} / {} 套模型；每项可独立显隐和选择",
                    runtime.models.len()
                ))
            }
            Command::AddAsset(bundle) => {
                let id = add_model(client, runtime, bundle);
                focus_model(runtime, id)?;
                Ok("已选择模型，加载结果见预览列表".into())
            }
            Command::ClearAssets => {
                clear_models(client, runtime)?;
                Ok("已清空预览模型".into())
            }
            Command::RemoveAsset(id) => {
                let index = runtime
                    .models
                    .iter()
                    .position(|model| model.id == id)
                    .ok_or("模型已移除")?;
                runtime.models[index].release(client)?;
                runtime.models.remove(index);
                if runtime.active_model == Some(id) {
                    runtime.active_model = None;
                    runtime.focus_bone = None;
                    if let Some(next) = runtime.models.first().map(|model| model.id) {
                        focus_model(runtime, next)?;
                    }
                }
                Ok("已移除所选模型".into())
            }
            Command::SelectModel(id) => {
                focus_model(runtime, id)?;
                Ok("已切换当前模型".into())
            }
            Command::ModelVisible { id, visible } => {
                let model = runtime
                    .models
                    .iter_mut()
                    .find(|model| model.id == id)
                    .ok_or("模型已移除")?;
                if visible && model.asset.is_none() {
                    return Err("模型未加载成功，请查看该项错误或重新加入预览".into());
                }
                model.visible = visible;
                Ok("已更新模型显隐".into())
            }
            Command::MeshVisible {
                model,
                mesh,
                visible,
            } => {
                model_asset(runtime, model)?.set_mesh_visible(mesh, visible)?;
                Ok("已更新子网格显隐".into())
            }
            Command::IsolateMesh { model, mesh } => {
                model_asset(runtime, model)?.isolate_mesh(mesh)?;
                Ok(format!("仅显示子网格 {mesh}"))
            }
            Command::ShowAllMeshes(model) => {
                model_asset(runtime, model)?.show_all_meshes();
                Ok("已显示该模型的全部子网格".into())
            }
            Command::BoneBinding {
                model,
                node,
                source,
            } => {
                model_asset(runtime, model)?.set_bone_binding(node, source)?;
                Ok(match source {
                    Some(source) => format!("节点 {node} 跟随节点 {source} 的世界姿态"),
                    None => format!("节点 {node} 已恢复原始骨架姿态"),
                })
            }
            Command::ClearBoneBindings(model) => {
                model_asset(runtime, model)?.clear_bone_bindings()?;
                Ok("已清除当前模型的骨骼姿态跟随".into())
            }
            Command::FocusAll => {
                focus_all(runtime)?;
                Ok("已聚焦所有可见模型".into())
            }
            Command::LoadScene(bundle) => {
                asset::preflight(&bundle)?;
                unload_scene(client, runtime)?;
                let scene = asset::NativeAsset::load(client, bundle)?;
                runtime.snapshot.scene = Some(scene.name().into());
                runtime.snapshot.scene_visible = true;
                runtime.scene = Some(scene);
                Ok("已载入场景资源".into())
            }
            Command::UnloadScene => {
                unload_scene(client, runtime)?;
                Ok("已卸载场景资源".into())
            }
            Command::SceneVisible(visible) => {
                runtime.snapshot.scene_visible = visible;
                Ok("已更新场景显示".into())
            }
            Command::LoadMotion(source) => {
                let model = active_model(runtime)?;
                let asset = model.asset.as_ref().ok_or("当前模型未加载成功")?;
                let (roots, nodes) = asset.animation_nodes(client)?;
                let motion = animation::NativeMotion::load(client, source, roots, nodes)?;
                if let Some(previous) = &mut model.motion {
                    previous.release(client)?;
                }
                model.motion = Some(motion);
                model.frame = 0.0;
                model.playing = true;
                runtime.last_frame = None;
                Ok("已为当前模型绑定所选动画".into())
            }
            Command::UnloadMotion => {
                active_model(runtime)?.unload_motion(client)?;
                Ok("已卸载当前模型动画".into())
            }
            Command::Playing(playing) => {
                active_model(runtime)?.playing = playing;
                runtime.last_frame = None;
                Ok(if playing {
                    "当前模型动画播放中"
                } else {
                    "当前模型动画已暂停"
                }
                .into())
            }
            Command::Seek(frame) => {
                let model = active_model(runtime)?;
                let frames = model.motion.as_ref().map_or(0.0, |motion| motion.frames());
                if !frame.is_finite() || frame < 0.0 || frame > frames {
                    return Err("动画帧超出范围".into());
                }
                model.playing = false;
                model.frame = frame;
                Ok(format!("帧 {frame:.2}"))
            }
            Command::Camera {
                distance,
                pitch,
                yaw,
            } => {
                if !distance.is_finite() || !pitch.is_finite() || !yaw.is_finite() {
                    return Err("镜头参数无效".into());
                }
                runtime.snapshot.distance = distance.clamp(1.0, 100_000.0);
                runtime.snapshot.pitch = pitch.clamp(-89.0, 89.0);
                runtime.snapshot.yaw = yaw;
                Ok("已更新预览镜头".into())
            }
            Command::FocusBone(index) => {
                if index.is_some_and(|index| {
                    !runtime
                        .snapshot
                        .bones
                        .iter()
                        .any(|bone| bone.index == index)
                }) {
                    return Err("骨骼编号无效".into());
                }
                if index.is_none()
                    && let Some(id) = runtime.active_model
                {
                    focus_model(runtime, id)?;
                }
                runtime.focus_bone = index;
                Ok("已更新镜头焦点".into())
            }
            Command::Exit => unreachable!(),
        }
    }
}

unsafe extern "C" fn bootstrap(task: usize) -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(usize) -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x008d25a0) };
        return unsafe { original(task) };
    };
    unsafe {
        if get::<u8>(task + 8) == 0 {
            let original: unsafe extern "C" fn(usize) -> i32 = transmute(state.bootstrap);
            let result = original(task);
            let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            runtime.initialized = true;
            runtime.snapshot.message = "工作台已就绪，请从资源浏览加载预览".into();
            eprintln!("workbench: resource pools initialized; empty preview, no quest or hunter");
            return result;
        }
        // Keep this task at its initialized stage. Do not enter login notices,
        // character creation, quest preparation, or stage loading.
        let queue: unsafe extern "C" fn(unsafe extern "C" fn() -> i32) -> i32 =
            transmute(state.client.address(0x10b75370));
        queue(render_preview)
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
        {
            let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
            if runtime.initialized && state.client.read::<i32>(0x1e866cb8) != 0 {
                let _ = clear_models(state.client, &mut runtime);
                let _ = unload_scene(state.client, &mut runtime);
            }
            for request in state.control.commands() {
                runtime.snapshot.message = command(state, &mut runtime, request)
                    .unwrap_or_else(|error| error)
                    .into();
            }
        }
        let original: unsafe extern "C" fn() -> i32 = transmute(state.dispatch);
        let result = original();
        let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
        runtime.snapshot.ready = ready(state, &runtime);
        refresh_snapshot(&mut runtime);
        state.control.publish(runtime.snapshot.clone());
        result
    }
}

/// Game input to parameter slots 90..92. 1000B190 converts this layout to
/// D3DLIGHT9: direction precedes position, and attenuation precedes range.
#[repr(C)]
struct Light {
    unknown_00: u32,
    diffuse: [f32; 4],
    specular: [f32; 4],
    ambient: [f32; 4],
    direction: [f32; 3],
    position: [f32; 3],
    attenuation: [f32; 3],
    range: f32,
    theta: f32,
    phi: f32,
    falloff: f32,
}

const _: () = assert!(std::mem::size_of::<Light>() == 104);

impl Light {
    fn directional(intensity: f32) -> Self {
        Self {
            unknown_00: 0,
            diffuse: [intensity, intensity, intensity, 1.0],
            specular: [0.0; 4],
            ambient: [0.0; 4],
            direction: [-0.4, -0.7, -0.6],
            position: [0.0; 3],
            attenuation: [1.0, 0.0, 0.0],
            range: 0.0,
            theta: 0.0,
            phi: 0.0,
            falloff: 0.0,
        }
    }
}

const WORLD: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

unsafe extern "C" fn render_preview() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    // Uploads can wait on this render worker while the task thread owns runtime.
    let Ok(mut runtime) = state.runtime.try_lock() else {
        return 0;
    };
    runtime.snapshot.camera = None;
    if !unsafe { ready(state, &runtime) } {
        state.control.publish(runtime.snapshot.clone());
        return 0;
    }
    let result = unsafe { render_frame(state, &mut runtime) };
    if let Err(error) = &result {
        runtime.snapshot.message = error.as_str().into();
    }
    refresh_snapshot(&mut runtime);
    // Camera and the pixel-rounded viewport must reach the overlay together,
    // including a cleared camera when a transient/empty viewport skips drawing.
    state.control.publish(runtime.snapshot.clone());
    i32::from(result.is_ok())
}

unsafe fn render_frame(state: &State, runtime: &mut Runtime) -> Result<(), String> {
    let requested = state.control.viewport();
    runtime.snapshot.viewport = requested;
    let pointer = unsafe { state.client.read::<*mut c_void>(0x1e811a3c) };
    let device =
        unsafe { IDirect3DDevice9::from_raw_borrowed(&pointer) }.ok_or("原生绘制设备尚未初始化")?;
    let mut viewport = unsafe { viewport::NativeViewport::begin(device, state.client, requested) }?;
    runtime.snapshot.viewport = viewport.normalized;
    let Some(pixels) = viewport.pixels else {
        runtime.last_frame = None;
        return viewport.restore();
    };
    if !runtime.rendered {
        runtime.rendered = true;
        eprintln!("workbench: empty preview render callback active");
    }
    let now = Instant::now();
    let elapsed = runtime
        .last_frame
        .replace(now)
        .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
    for model in &mut runtime.models {
        if model.playing
            && let Some(motion) = &model.motion
            && motion.frames() > 0.0
        {
            model.frame = (model.frame + elapsed.min(0.1) * 30.0).rem_euclid(motion.frames());
        }
    }
    let snapshot = &runtime.snapshot;
    let target = runtime
        .focus_bone
        .and_then(|index| snapshot.bones.iter().find(|bone| bone.index == index))
        .map_or(runtime.center, |bone| bone.position);
    let pitch = snapshot.pitch.to_radians();
    let yaw = snapshot.yaw.to_radians();
    let radius = snapshot.distance;
    let camera = Camera {
        eye: [
            target[0] + radius * pitch.cos() * yaw.sin(),
            target[1] + radius * pitch.sin(),
            target[2] + radius * pitch.cos() * yaw.cos(),
        ],
        target,
        up: [0.0, 1.0, 0.0],
        fov_y: std::f32::consts::FRAC_PI_3,
        aspect: pixels.aspect(),
    };
    let (view, projection) = camera.matrices(1.0, 200_000.0);
    let parameter: unsafe extern "fastcall" fn(usize, u32) -> i32 =
        unsafe { transmute(state.client.address(0x1000c7d0)) };
    unsafe {
        // Same view/projection slots used by native 10BAEE10 and 1000D870.
        parameter(view.as_ptr() as usize, 22);
        parameter(projection.as_ptr() as usize, 23);
        // Supply neutral inspection light without loading map lighting data.
        parameter(0xff80_8080, 14);
        for (slot, intensity) in [(90, 0.8), (91, 0.0), (92, 0.0)] {
            let light = Light::directional(intensity);
            parameter(&light as *const Light as usize, slot);
        }
        // Projection matrix updates do not initialize the native batch culler.
        // Match 10BAF300: XMM1 = far, stack = fov and near. The viewport scope
        // supplies the same temporary global aspect used by camera projection.
        update_frustum(
            state.client.address(0x10006b10),
            camera.fov_y,
            1.0,
            200_000.0,
        );
        if runtime.snapshot.scene_visible
            && let Some(scene) = &mut runtime.scene
            && let Err(error) = scene.draw(state.client, &WORLD, 0.0)
        {
            runtime.snapshot.message = error.into();
        }
        for model in &mut runtime.models {
            if model.visible
                && let Some(asset) = &mut model.asset
            {
                if let Err(error) = asset.draw(state.client, &WORLD, model.frame) {
                    if model.error.as_deref() != Some(error.as_str()) {
                        eprintln!("workbench: model {} draw failed: {error}", model.id);
                    }
                    model.error = Some(error.into());
                } else {
                    let bones = asset.bones(state.client);
                    if model.bones.is_empty() && !bones.is_empty() {
                        eprintln!(
                            "workbench: model {} rendered with {} skeleton nodes",
                            model.id,
                            bones.len()
                        );
                    }
                    model.bones = Arc::new(bones);
                }
            }
        }
    }
    viewport.restore()?;
    runtime.snapshot.camera = Some(camera);
    if !runtime.viewport_reported && viewport.normalized != crate::preview::Viewport::default() {
        runtime.viewport_reported = true;
        eprintln!(
            "workbench: central viewport {}x{} at {},{}; aspect {:.5}",
            pixels.width,
            pixels.height,
            pixels.x,
            pixels.y,
            pixels.aspect()
        );
    }
    Ok(())
}

#[unsafe(naked)]
unsafe extern "C" fn update_frustum(_target: usize, _fov: f32, _near: f32, _far: f32) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp,esp",
        "movss xmm1,[ebp+20]",
        "push dword ptr [ebp+16]",
        "push dword ptr [ebp+12]",
        "call dword ptr [ebp+8]",
        "add esp,8",
        "pop ebp",
        "ret",
    );
}
