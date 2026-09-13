//! Resource preview inside the native graphics loop, without a quest or hunter.

mod animation;
mod asset;
mod effect_resources;
mod guides;
mod refresh;
mod skeleton;
mod textures;
mod viewport;
mod window;

use crate::inspect::Kind;
use crate::preview::effects::{self, Effects};
use crate::preview::{
    AssetBundle, Bone, Camera, Command, Control, LoadedModel, LoadedMotion, LoadedResource,
    LoadedSkeleton, PlaybackTrack, ResourceRef, Snapshot,
};
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
    window: window::Hooks,
    runtime: Mutex<Runtime>,
}

#[derive(Default)]
struct Runtime {
    initialized: bool,
    rendered: bool,
    viewport_reported: bool,
    snapshot: Snapshot,
    focus_bone: Option<(u64, usize)>,
    center: [f32; 3],
    models: Vec<Model>,
    next_model_id: u64,
    resources: Vec<LoadedResource>,
    next_resource_id: u64,
    skeletons: Vec<SkeletonResource>,
    motions: Vec<MotionResource>,
    next_motion_id: u64,
    next_motion_activation: u64,
    fx: effect_resources::Registry,
    last_frame: Option<Instant>,
}

struct Model {
    id: u64,
    identity: Option<crate::metadata::EquipmentModel>,
    definition: AssetBundle,
    bundle: AssetBundle,
    asset: Option<asset::NativeAsset>,
    visible: bool,
    error: Option<Arc<str>>,
    motions: Vec<BoundMotion>,
    frame: f32,
    bones: Arc<Vec<Bone>>,
    effects: Effects,
}

struct BoundMotion {
    resource: u64,
    native: animation::NativeMotion,
}

unsafe fn release_motions(client: Client, motions: &mut Vec<BoundMotion>) -> Result<(), String> {
    for motion in motions.iter_mut() {
        unsafe { motion.native.release(client) }?;
    }
    motions.clear();
    Ok(())
}

struct SkeletonResource {
    id: u64,
    source: ResourceRef,
    native: Option<skeleton::Standalone>,
    motions: Vec<BoundMotion>,
    bones: Arc<Vec<Bone>>,
    bindings: Arc<Vec<Option<usize>>>,
    error: Option<Arc<str>>,
}

impl SkeletonResource {
    unsafe fn release(&mut self, client: Client) -> Result<(), String> {
        unsafe { release_motions(client, &mut self.motions) }?;
        self.native = None;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MotionTarget {
    Model(u64),
    Skeleton(u64),
}

#[derive(Clone)]
struct MotionResource {
    id: u64,
    source: ResourceRef,
    enabled: bool,
    frames: f32,
    frame: f32,
    activation: u64,
}

impl MotionResource {
    fn seek(&mut self, frame: f32) -> Result<(), String> {
        if !frame.is_finite() || frame < 0.0 || frame > self.frames {
            return Err("动画轨道步数超出范围".into());
        }
        self.frame = frame;
        Ok(())
    }
}

impl Model {
    fn uses_skeleton(&self, source: &ResourceRef) -> bool {
        self.bundle
            .skeleton
            .as_ref()
            .is_some_and(|skeleton| skeleton.same_instance(source))
    }

    unsafe fn prepare_resources(
        &self,
        client: Client,
        bundle: &AssetBundle,
    ) -> Result<(asset::NativeAsset, effects::Target), String> {
        let mut replacement = unsafe { asset::NativeAsset::load(client, bundle.clone()) }?;
        let prepared = (|| {
            let target = unsafe { replacement.effect_target(client) }?;
            if let Some(previous) = &self.asset {
                for mesh in previous.meshes().iter().take(replacement.meshes().len()) {
                    replacement.set_mesh_visible(mesh.index, mesh.visible)?;
                }
                if self
                    .bundle
                    .skeleton
                    .as_ref()
                    .zip(bundle.skeleton.as_ref())
                    .is_some_and(|(a, b)| a.same_origin(b))
                {
                    replacement.set_bone_bindings(refresh::compatible_bindings(
                        &previous.bone_bindings(),
                        replacement.bone_bindings().len(),
                    ))?;
                }
            }
            Ok::<_, String>(target)
        })();
        let target = match prepared {
            Ok(target) => target,
            Err(error) => {
                unsafe { replacement.release(client) }?;
                return Err(error);
            }
        };
        Ok((replacement, target))
    }

    unsafe fn update_resources(
        &mut self,
        client: Client,
        bundle: AssetBundle,
    ) -> Result<(), String> {
        let (mut replacement, target) = unsafe { self.prepare_resources(client, &bundle) }?;
        let released = unsafe {
            (|| {
                release_motions(client, &mut self.motions)?;
                if let Some(previous) = &mut self.asset {
                    previous.release(client)?;
                }
                Ok::<_, String>(())
            })()
        };
        if let Err(error) = released {
            unsafe { replacement.release(client) }?;
            return Err(error);
        }
        self.asset = Some(replacement);
        self.bundle = bundle;
        self.bones = Arc::default();
        self.error = None;
        self.effects.target = target;
        self.effects.snapshot = Arc::new(self.effects.sample(self.frame).bindings);
        Ok(())
    }

    unsafe fn release(&mut self, client: Client) -> Result<(), String> {
        unsafe {
            release_motions(client, &mut self.motions)?;
        }
        if let Some(asset) = &mut self.asset {
            unsafe {
                asset.release(client)?;
            }
        }
        self.asset = None;
        self.bones = Arc::new(Vec::new());
        self.effects = Effects::default();
        Ok(())
    }
}

impl State {
    pub unsafe fn prepare_release(&mut self) -> Result<(), String> {
        unsafe {
            self.window.restore(self.client)?;
            self.module.release()
        }
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
    let window = unsafe { window::prepare(client, &mut hooks) }?;
    let guard = unsafe {
        hooks.install(State {
            module: retained,
            client,
            control,
            bootstrap: bootstrap as usize,
            dispatch: dispatch as usize,
            window,
            runtime: Mutex::new(Runtime::default()),
        })
    }?;
    unsafe { window::activate(client) };
    Ok(guard)
}

unsafe fn ready(state: &State, runtime: &Runtime) -> bool {
    runtime.initialized
        && [0x1ed528d0, 0x1ed528c4, 0x1ed528c8, 0x1ed528cc]
            .into_iter()
            .all(|address| unsafe { state.client.read::<usize>(address) != 0 })
}

unsafe fn clear_models(client: Client, runtime: &mut Runtime) -> Result<(), String> {
    runtime.fx.capture(&runtime.models);
    // Keep every source alive until all native release calls have completed.
    for model in &mut runtime.models {
        unsafe {
            model.release(client)?;
        }
    }
    runtime.models.clear();
    runtime.last_frame = None;
    Ok(())
}

fn model_asset(runtime: &mut Runtime, id: u64) -> Result<&mut asset::NativeAsset, String> {
    runtime
        .models
        .iter_mut()
        .find(|model| model.id == id)
        .ok_or("模型已移除")?
        .asset
        .as_mut()
        .ok_or_else(|| "模型未加载成功，请查看该项错误或重新加载".into())
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
    for skeleton in &runtime.skeletons {
        if runtime
            .resources
            .iter()
            .any(|resource| resource.id == skeleton.id && resource.enabled)
        {
            for bone in skeleton.bones.iter() {
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(bone.position[axis]);
                    maximum[axis] = maximum[axis].max(bone.position[axis]);
                }
                found = true;
            }
        }
    }
    if !found {
        return Err("没有可见的模型或骨架".into());
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

fn focus_skeleton(runtime: &mut Runtime, id: u64) -> Result<(), String> {
    let skeleton = runtime
        .skeletons
        .iter()
        .find(|skeleton| skeleton.id == id)
        .ok_or("骨架已卸载")?;
    let first = skeleton.bones.first().ok_or_else(|| {
        skeleton
            .error
            .as_deref()
            .unwrap_or("骨架尚无可用的节点位置")
            .to_owned()
    })?;
    let mut minimum = first.position;
    let mut maximum = first.position;
    for bone in skeleton.bones.iter().skip(1) {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(bone.position[axis]);
            maximum[axis] = maximum[axis].max(bone.position[axis]);
        }
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

fn edit_skeleton_binding(
    runtime: &mut Runtime,
    id: u64,
    edit: Option<(usize, Option<usize>)>,
) -> Result<(), String> {
    let index = runtime
        .skeletons
        .iter()
        .position(|skeleton| skeleton.id == id)
        .ok_or("骨架已卸载")?;
    let source = runtime.skeletons[index].source.clone();
    let previous = runtime.skeletons[index].bindings.clone();
    let bindings = if let Some(native) = &mut runtime.skeletons[index].native {
        if let Some((node, source)) = edit {
            native.set_binding(node, source)?;
        } else {
            native.clear_bindings();
        }
        native.bindings()
    } else {
        let asset = runtime
            .models
            .iter_mut()
            .find_map(|model| {
                model
                    .uses_skeleton(&source)
                    .then_some(model.asset.as_mut())
                    .flatten()
            })
            .ok_or("请先启用并成功加载骨架")?;
        if let Some((node, source)) = edit {
            asset.set_bone_binding(node, source)?;
        } else {
            asset.clear_bone_bindings()?;
        }
        asset.bone_bindings()
    };
    let result = (|| {
        for model in &mut runtime.models {
            if model.uses_skeleton(&source)
                && let Some(asset) = &mut model.asset
            {
                asset.set_bone_bindings(bindings.clone())?;
            }
        }
        if let Some(native) = &mut runtime.skeletons[index].native {
            native.set_bindings(bindings.clone())?;
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        for model in &mut runtime.models {
            if model.uses_skeleton(&source)
                && let Some(asset) = &mut model.asset
            {
                asset.set_bone_bindings(previous.clone())?;
            }
        }
        if let Some(native) = &mut runtime.skeletons[index].native {
            native.set_bindings(previous)?;
        }
        return Err(error);
    }
    runtime.skeletons[index].bindings = bindings;
    Ok(())
}

fn register_resource(runtime: &mut Runtime, source: ResourceRef) {
    if source.same_source(&ResourceRef::white_texture()) {
        return;
    }
    if let Some(entry) = runtime
        .resources
        .iter_mut()
        .find(|entry| entry.source.same_instance(&source))
    {
        entry.enabled = true;
        return;
    }
    runtime.next_resource_id = runtime.next_resource_id.wrapping_add(1);
    runtime.resources.push(LoadedResource {
        id: runtime.next_resource_id,
        source,
        enabled: true,
    });
}

unsafe fn reconcile_resources(client: Client, runtime: &mut Runtime) -> Result<(), String> {
    runtime.fx.capture(&runtime.models);
    for model in &mut runtime.models {
        let bundle = model.definition.loaded_from(&runtime.resources);
        if !model.bundle.same_source(&bundle) {
            unsafe { model.update_resources(client, bundle) }?;
        }
    }
    unsafe {
        reconcile_skeletons(client, runtime)?;
        reconcile_motions(client, runtime)?;
        reconcile_effects(client, runtime)
    }
}

unsafe fn reconcile_effects(client: Client, runtime: &mut Runtime) -> Result<(), String> {
    unsafe {
        runtime
            .fx
            .reconcile(client, &mut runtime.models, |binding, model| {
                model
                    .identity
                    .is_some_and(|identity| identity.matches(binding))
            })
    }
}

unsafe fn reconcile_skeletons(client: Client, runtime: &mut Runtime) -> Result<(), String> {
    if let Some((id, _)) = runtime.focus_bone
        && !runtime.resources.iter().any(|resource| resource.id == id)
    {
        runtime.center = camera_target(runtime);
        runtime.focus_bone = None;
    }
    let mut index = 0;
    while let Some(skeleton) = runtime.skeletons.get_mut(index) {
        if runtime
            .resources
            .iter()
            .any(|resource| resource.id == skeleton.id)
        {
            index += 1;
        } else {
            unsafe { skeleton.release(client) }?;
            runtime.skeletons.remove(index);
        }
    }
    for resource in runtime
        .resources
        .iter()
        .filter(|resource| resource.source.kind() == Kind::Fskl)
    {
        if !runtime
            .skeletons
            .iter()
            .any(|skeleton| skeleton.id == resource.id)
        {
            runtime.skeletons.push(SkeletonResource {
                id: resource.id,
                source: resource.source.clone(),
                native: None,
                motions: Vec::new(),
                bones: Arc::default(),
                bindings: Arc::default(),
                error: None,
            });
        }
        let skeleton = runtime
            .skeletons
            .iter_mut()
            .find(|skeleton| skeleton.id == resource.id)
            .unwrap();
        if !resource.enabled {
            unsafe { skeleton.release(client) }?;
            continue;
        }
        let mut models = runtime
            .models
            .iter_mut()
            .filter(|model| model.asset.is_some() && model.uses_skeleton(&resource.source))
            .peekable();
        if models.peek().is_some() {
            // Multiple model copies still share one logical FSKL resource.
            unsafe { skeleton.release(client) }?;
            for (index, model) in models.enumerate() {
                let asset = model.asset.as_mut().unwrap();
                if skeleton.bindings.is_empty() {
                    skeleton.bindings = asset.bone_bindings();
                } else {
                    skeleton.bindings = refresh::compatible_bindings(
                        &skeleton.bindings,
                        asset.bone_bindings().len(),
                    );
                    asset.set_bone_bindings(skeleton.bindings.clone())?;
                }
                if model.bones.is_empty() {
                    unsafe { asset.update_skeleton(client, &[], &WORLD) }?;
                    model.bones = Arc::new(unsafe { asset.bones(client) });
                }
                if index == 0 {
                    skeleton.bones = model.bones.clone();
                }
            }
            skeleton.error = None;
            continue;
        }
        if skeleton.native.is_none() {
            match unsafe { skeleton::Standalone::load(client, skeleton.source.clone()) } {
                Ok(mut native) => {
                    if !skeleton.bindings.is_empty() {
                        skeleton.bindings = refresh::compatible_bindings(
                            &skeleton.bindings,
                            native.bindings().len(),
                        );
                        native.set_bindings(skeleton.bindings.clone())?;
                        unsafe { native.update(client, &[], &WORLD) }?;
                    }
                    skeleton.bindings = native.bindings();
                    skeleton.bones = Arc::new(native.bones());
                    skeleton.native = Some(native);
                    skeleton.error = None;
                }
                Err(error) => skeleton.error = Some(error.into()),
            }
        }
    }
    Ok(())
}

unsafe fn clear_skeletons(client: Client, runtime: &mut Runtime) -> Result<(), String> {
    for skeleton in &mut runtime.skeletons {
        unsafe { skeleton.release(client) }?;
    }
    runtime.skeletons.clear();
    Ok(())
}

struct MotionBinding {
    resource: u64,
    skeleton: u64,
    target: MotionTarget,
    nodes: Vec<usize>,
    activation: u64,
}

fn select_motion_bindings(mut candidates: Vec<MotionBinding>) -> (Vec<MotionBinding>, Vec<u64>) {
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.activation));
    let mut selected: Vec<MotionBinding> = Vec::new();
    let mut stopped = Vec::new();
    for candidate in candidates {
        if stopped.contains(&candidate.resource) {
            continue;
        }
        if selected.iter().any(|newer| {
            newer.resource != candidate.resource
                && newer.skeleton == candidate.skeleton
                && newer.target == candidate.target
                && newer
                    .nodes
                    .iter()
                    .any(|node| candidate.nodes.contains(node))
        }) {
            stopped.push(candidate.resource);
            selected.retain(|binding| binding.resource != candidate.resource);
        } else {
            selected.push(candidate);
        }
    }
    (selected, stopped)
}

fn target_motions(runtime: &Runtime, target: MotionTarget) -> Result<&Vec<BoundMotion>, String> {
    match target {
        MotionTarget::Model(id) => runtime
            .models
            .iter()
            .find(|model| model.id == id)
            .map(|model| &model.motions),
        MotionTarget::Skeleton(id) => runtime
            .skeletons
            .iter()
            .find(|skeleton| skeleton.id == id)
            .map(|skeleton| &skeleton.motions),
    }
    .ok_or_else(|| "动画目标已移除".into())
}

fn target_motions_mut(
    runtime: &mut Runtime,
    target: MotionTarget,
) -> Result<&mut Vec<BoundMotion>, String> {
    match target {
        MotionTarget::Model(id) => runtime
            .models
            .iter_mut()
            .find(|model| model.id == id)
            .map(|model| &mut model.motions),
        MotionTarget::Skeleton(id) => runtime
            .skeletons
            .iter_mut()
            .find(|skeleton| skeleton.id == id)
            .map(|skeleton| &mut skeleton.motions),
    }
    .ok_or_else(|| "动画目标已移除".into())
}

unsafe fn restore_motion_bindings(runtime: &Runtime) -> Result<(), String> {
    for motions in runtime
        .models
        .iter()
        .map(|model| &model.motions)
        .chain(runtime.skeletons.iter().map(|skeleton| &skeleton.motions))
    {
        for motion in motions {
            unsafe { motion.native.activate() }?;
        }
    }
    Ok(())
}

unsafe fn load_bound_motion(
    client: Client,
    runtime: &Runtime,
    target: MotionTarget,
    source: ResourceRef,
) -> Result<animation::NativeMotion, String> {
    let (roots, nodes) = match target {
        MotionTarget::Model(id) => {
            let asset = runtime
                .models
                .iter()
                .find(|model| model.id == id)
                .and_then(|model| model.asset.as_ref())
                .ok_or("动画目标模型已释放")?;
            unsafe { asset.animation_nodes(client) }?
        }
        MotionTarget::Skeleton(id) => {
            let skeleton = runtime
                .skeletons
                .iter()
                .find(|skeleton| skeleton.id == id)
                .and_then(|skeleton| skeleton.native.as_ref())
                .ok_or("动画目标骨架已释放")?;
            skeleton.animation_nodes()
        }
    };
    unsafe { animation::NativeMotion::load(client, source, roots, nodes) }
}

unsafe fn reconcile_motions(client: Client, runtime: &mut Runtime) -> Result<(), String> {
    let mut candidates = Vec::new();
    for motion in runtime.motions.iter().filter(|motion| motion.enabled) {
        let mut matching = Vec::new();
        for skeleton in &runtime.skeletons {
            if !motion.source.compatible_scope(&skeleton.source) {
                continue;
            }
            let mut instances = Vec::new();
            let mut complete = true;
            for model in runtime
                .models
                .iter()
                .filter(|model| model.uses_skeleton(&skeleton.source))
            {
                let Some(asset) = &model.asset else { continue };
                let nodes = unsafe {
                    asset.animation_nodes(client).and_then(|(roots, nodes)| {
                        animation::NativeMotion::binding_nodes(&motion.source, roots, nodes)
                    })
                };
                match nodes {
                    Ok(nodes) => instances.push(MotionBinding {
                        resource: motion.id,
                        skeleton: skeleton.id,
                        target: MotionTarget::Model(model.id),
                        nodes,
                        activation: motion.activation,
                    }),
                    Err(_) => {
                        complete = false;
                        break;
                    }
                }
            }
            if complete && let Some(native) = &skeleton.native {
                let (roots, nodes) = native.animation_nodes();
                match unsafe {
                    animation::NativeMotion::binding_nodes(&motion.source, roots, nodes)
                } {
                    Ok(nodes) => instances.push(MotionBinding {
                        resource: motion.id,
                        skeleton: skeleton.id,
                        target: MotionTarget::Skeleton(skeleton.id),
                        nodes,
                        activation: motion.activation,
                    }),
                    Err(_) => complete = false,
                }
            }
            if complete {
                matching.extend(instances);
            }
        }
        // Multiple native model copies of one FSKL share a logical target.
        // Different loaded FSKL resources still require a unique match.
        if let Some(first) = matching.first()
            && matching
                .iter()
                .all(|binding| binding.skeleton == first.skeleton)
        {
            candidates.extend(matching);
        }
    }
    let (selected, stopped) = select_motion_bindings(candidates);
    let mut prepared: Vec<(MotionTarget, BoundMotion)> = Vec::new();
    for binding in &selected {
        let source = &runtime
            .motions
            .iter()
            .find(|motion| motion.id == binding.resource)
            .ok_or("已加载动画记录失效")?
            .source;
        if target_motions(runtime, binding.target)?
            .iter()
            .any(|motion| {
                motion.resource == binding.resource
                    && motion.native.source.same_instance(source)
                    && motion
                        .native
                        .target_nodes()
                        .eq(binding.nodes.iter().copied())
            })
        {
            continue;
        }
        match unsafe { load_bound_motion(client, runtime, binding.target, source.clone()) } {
            Ok(native) => prepared.push((
                binding.target,
                BoundMotion {
                    resource: binding.resource,
                    native,
                },
            )),
            Err(error) => {
                for (_, motion) in &mut prepared {
                    unsafe { motion.native.release(client) }?;
                }
                unsafe { restore_motion_bindings(runtime) }?;
                return Err(error);
            }
        }
    }
    for (target, motions) in runtime
        .models
        .iter_mut()
        .map(|model| (MotionTarget::Model(model.id), &mut model.motions))
        .chain(
            runtime
                .skeletons
                .iter_mut()
                .map(|skeleton| (MotionTarget::Skeleton(skeleton.id), &mut skeleton.motions)),
        )
    {
        let mut motion_index = 0;
        while let Some(motion) = motions.get_mut(motion_index) {
            if selected
                .iter()
                .any(|binding| binding.target == target && binding.resource == motion.resource)
                && !prepared.iter().any(|(replacement_target, replacement)| {
                    *replacement_target == target && replacement.resource == motion.resource
                })
            {
                motion_index += 1;
            } else {
                unsafe { motion.native.release(client) }?;
                motions.remove(motion_index);
            }
        }
    }
    for (target, motion) in prepared {
        target_motions_mut(runtime, target)?.push(motion);
    }
    for motion in &mut runtime.motions {
        if stopped.contains(&motion.id) {
            motion.enabled = false;
        }
    }
    Ok(())
}

fn motion_frames(motions: &[BoundMotion], resources: &[MotionResource]) -> Vec<(usize, f32)> {
    motions
        .iter()
        .flat_map(|bound| {
            let frame = resources
                .iter()
                .find(|motion| motion.id == bound.resource)
                .map_or(0.0, |motion| motion.frame);
            bound.native.target_nodes().map(move |node| (node, frame))
        })
        .collect()
}

fn motion_resource(runtime: &mut Runtime, id: u64) -> Result<&mut MotionResource, String> {
    runtime
        .motions
        .iter_mut()
        .find(|motion| motion.id == id)
        .ok_or_else(|| "动画已卸载".into())
}

unsafe fn add_model(client: Client, runtime: &mut Runtime, definition: AssetBundle) -> u64 {
    runtime.fx.capture(&runtime.models);
    let bundle = definition.loaded_from(&runtime.resources);
    if let Some(existing) = runtime
        .models
        .iter_mut()
        .find(|model| model.definition.same_instance(&definition))
    {
        if existing.asset.is_none() || existing.error.is_some() {
            if let Err(error) = unsafe { existing.release(client) } {
                existing.error = Some(error.into());
                return existing.id;
            }
            match unsafe { asset::NativeAsset::load(client, bundle.clone()) } {
                Ok(asset) => {
                    existing.bundle = bundle;
                    existing.asset = Some(asset);
                    existing.error = None;
                    existing.visible = true;
                }
                Err(error) => existing.error = Some(error.into()),
            }
        } else if !existing.bundle.same_source(&bundle) {
            if let Err(error) = unsafe { existing.update_resources(client, bundle) } {
                existing.error = Some(error.into());
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
        identity: definition.model.equipment_model(),
        definition,
        bundle,
        visible: true,
        asset,
        error,
        motions: Vec::new(),
        frame: 0.0,
        bones: Arc::new(Vec::new()),
        effects: Effects::default(),
    });
    id
}

fn refresh_snapshot(runtime: &mut Runtime) {
    runtime.snapshot.loaded_effects = Arc::new(runtime.fx.snapshot(&runtime.models));
    runtime.snapshot.resources = Arc::new(runtime.resources.clone());
    runtime.snapshot.skeletons = Arc::new(
        runtime
            .skeletons
            .iter()
            .map(|skeleton| LoadedSkeleton {
                id: skeleton.id,
                bones: skeleton.bones.clone(),
                bone_bindings: skeleton.bindings.clone(),
                error: skeleton.error.clone(),
            })
            .collect(),
    );
    runtime.snapshot.motions = Arc::new(
        runtime
            .motions
            .iter()
            .map(|motion| LoadedMotion {
                id: motion.id,
                source: motion.source.clone(),
                enabled: motion.enabled,
                frames: motion.frames,
                frame: Some(motion.frame),
                skeleton: runtime.skeletons.iter().find_map(|skeleton| {
                    (skeleton
                        .motions
                        .iter()
                        .any(|bound| bound.resource == motion.id)
                        || runtime.models.iter().any(|model| {
                            model.uses_skeleton(&skeleton.source)
                                && model
                                    .motions
                                    .iter()
                                    .any(|bound| bound.resource == motion.id)
                        }))
                    .then_some(skeleton.id)
                }),
            })
            .collect(),
    );
    runtime.snapshot.models = Arc::new(
        runtime
            .models
            .iter()
            .map(|model| LoadedModel {
                id: model.id,
                resources: model.bundle.clone(),
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
}

unsafe fn load_model(
    client: Client,
    runtime: &mut Runtime,
    definition: AssetBundle,
) -> Result<(), String> {
    let id = unsafe { add_model(client, runtime, definition) };
    unsafe {
        reconcile_skeletons(client, runtime)?;
        reconcile_motions(client, runtime)?;
        reconcile_effects(client, runtime)?;
    }
    let model = runtime
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or("模型已移除")?;
    let asset = model.asset.as_ref().ok_or_else(|| {
        model
            .error
            .as_deref()
            .unwrap_or("模型未加载成功")
            .to_owned()
    })?;
    (runtime.center, runtime.snapshot.distance) = asset.framing();
    runtime.focus_bone = None;
    Ok(())
}

unsafe fn load_motion(
    client: Client,
    runtime: &mut Runtime,
    source: ResourceRef,
) -> Result<(), String> {
    let previous = runtime.motions.clone();
    runtime.next_motion_activation = runtime.next_motion_activation.wrapping_add(1);
    if let Some(motion) = runtime
        .motions
        .iter_mut()
        .find(|motion| motion.source.same_instance(&source))
    {
        motion.enabled = true;
        motion.activation = runtime.next_motion_activation;
    } else {
        let frames = animation::NativeMotion::duration(&source)?;
        runtime.next_motion_id = runtime.next_motion_id.wrapping_add(1);
        runtime.motions.push(MotionResource {
            id: runtime.next_motion_id,
            source,
            enabled: true,
            frames,
            frame: 0.0,
            activation: runtime.next_motion_activation,
        });
    }
    if let Err(error) = unsafe { reconcile_motions(client, runtime) } {
        runtime.motions = previous;
        unsafe { reconcile_motions(client, runtime) }?;
        return Err(error);
    }
    runtime.snapshot.playing = true;
    runtime.last_frame = None;
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
            runtime.fx.clear(&mut runtime.models);
            clear_models(client, runtime)?;
            clear_skeletons(client, runtime)?;
            put(client.address(0x1e866cb8), 1_i32);
        }
        return Ok("正在结束资源工作台".into());
    }
    if matches!(command, Command::ToggleFullscreen) {
        // 114D6580 consumes this request on the native device thread and runs
        // the existing 114D5420 window/fullscreen transition and reset path.
        unsafe { put(client.address(0x1e866cc4), 1_i32) };
        return Ok("正在切换全屏 / 窗口模式".into());
    }
    if !unsafe { ready(state, runtime) } {
        return Err("原生资源池尚未初始化".into());
    }
    unsafe {
        match command {
            Command::RefreshDocument { path, document } => {
                refresh::document(client, runtime, &path, document)?;
                Ok("已更新已加载资源的预览".into())
            }
            Command::LoadResource(source) => {
                source.bytes()?;
                let mut resources = source.loadable_resources();
                if resources.is_empty() {
                    return Err("所选范围内没有可加载资源".into());
                }
                // Register the selected skeletons/images together. A TXB load
                // must not rebuild an existing model once for each image.
                resources.sort_by_key(|source| match source.kind() {
                    Kind::Fskl | Kind::Png | Kind::Dds => 0,
                    Kind::Fmod => 1,
                    _ => 2,
                });
                let count = resources.len();
                let shape_sources: Vec<_> = resources
                    .iter()
                    .filter(|resource| matches!(resource.kind(), Kind::Fmod | Kind::Fskl))
                    .cloned()
                    .collect();
                let mut errors = Vec::new();
                let (dependencies, resources): (Vec<_>, Vec<_>) =
                    resources.into_iter().partition(|source| {
                        matches!(source.kind(), Kind::Fskl | Kind::Png | Kind::Dds)
                    });
                if !dependencies.is_empty() {
                    let previous = runtime.resources.clone();
                    for resource in dependencies {
                        register_resource(runtime, resource);
                    }
                    if let Err(error) = reconcile_resources(client, runtime) {
                        runtime.resources = previous;
                        reconcile_resources(client, runtime)?;
                        errors.push(error);
                    }
                }
                // Definitions resolve against the complete selected dependency
                // set; animations then see all models loaded by this operation.
                let mut named_bundles = None;
                for resource in resources {
                    let loaded = match resource.kind() {
                        Kind::Fmod => {
                            let named = named_bundles.get_or_insert_with(|| {
                                AssetBundle::find_with_nodes(resource.document.clone()).0
                            });
                            load_model(client, runtime, AssetBundle::from_source(resource, named))
                        }
                        Kind::Motion => load_motion(client, runtime, resource),
                        kind if effects::is_binding(kind) || effects::is_definition(kind) => {
                            runtime.fx.load(resource).and_then(|_| {
                                reconcile_effects(client, runtime)?;
                                runtime.snapshot.playing = true;
                                runtime.last_frame = None;
                                Ok(())
                            })
                        }
                        _ => Err("所选节点不是可加载的完整资源".into()),
                    };
                    if let Err(error) = loaded {
                        errors.push(error);
                    }
                }
                if errors.is_empty() {
                    if shape_sources.len() > 1 {
                        focus_all(runtime)?;
                    } else if let Some(source) = shape_sources.first()
                        && source.kind() == Kind::Fskl
                        && let Some(id) = runtime
                            .resources
                            .iter()
                            .find(|resource| resource.source.same_instance(source))
                            .map(|resource| resource.id)
                    {
                        focus_skeleton(runtime, id)?;
                    }
                    Ok(format!("已加载所选范围内的 {count} 个资源"))
                } else {
                    Err(errors.join("；"))
                }
            }
            Command::ResourceEnabled { id, enabled } => {
                let index = runtime
                    .resources
                    .iter()
                    .position(|entry| entry.id == id)
                    .ok_or("资源已卸载")?;
                let previous = runtime.resources[index].enabled;
                runtime.resources[index].enabled = enabled;
                if let Err(error) = reconcile_resources(client, runtime) {
                    runtime.resources[index].enabled = previous;
                    reconcile_resources(client, runtime)?;
                    return Err(error);
                }
                Ok("已更新资源启用状态".into())
            }
            Command::ClearResources(kind) => {
                let previous = runtime.resources.clone();
                runtime.resources.retain(|entry| !entry.in_category(kind));
                if let Err(error) = reconcile_resources(client, runtime) {
                    runtime.resources = previous;
                    reconcile_resources(client, runtime)?;
                    return Err(error);
                }
                Ok("已清空所选分类的资源".into())
            }
            Command::RemoveResource(id) => {
                let index = runtime
                    .resources
                    .iter()
                    .position(|entry| entry.id == id)
                    .ok_or("资源已移除")?;
                let removed = runtime.resources.remove(index);
                if let Err(error) = reconcile_resources(client, runtime) {
                    runtime.resources.insert(index, removed);
                    reconcile_resources(client, runtime)?;
                    return Err(error);
                }
                Ok("已移除资源并更新关联模型".into())
            }
            Command::BindEffect { binding, model } => {
                runtime.fx.set_target(binding, model)?;
                reconcile_effects(client, runtime)?;
                Ok("已更新特效的手动绑定".into())
            }
            Command::AutoBindEffect(binding) => {
                runtime.fx.set_automatic(binding)?;
                reconcile_effects(client, runtime)?;
                Ok("已按特效自身的模型 ID 更新绑定".into())
            }
            Command::EffectEnabled { binding, enabled } => {
                runtime.fx.set_enabled(binding, enabled)?;
                reconcile_effects(client, runtime)?;
                Ok("已更新特效启用状态".into())
            }
            Command::RemoveEffect(binding) => {
                runtime.fx.remove(&mut runtime.models, binding)?;
                Ok("已卸载所选特效资源".into())
            }
            Command::ClearLoadedEffects => {
                runtime.fx.clear(&mut runtime.models);
                Ok("已清空特效资源".into())
            }
            Command::TriggerEffectDefinition { binding, slot } => {
                runtime.fx.trigger(&mut runtime.models, binding, slot)?;
                runtime.snapshot.playing = true;
                runtime.last_frame = None;
                Ok("已触发所选特效定义".into())
            }
            Command::StopEffectDefinition { binding, slot } => {
                runtime.fx.stop(&mut runtime.models, binding, slot)?;
                Ok("已停止所选特效定义".into())
            }
            Command::RemoveEffectDefinition { binding, slot } => {
                runtime
                    .fx
                    .remove_definition(&mut runtime.models, binding, slot)?;
                Ok("已移除所选特效定义".into())
            }
            Command::ClearAssets => {
                clear_models(client, runtime)?;
                reconcile_skeletons(client, runtime)?;
                reconcile_motions(client, runtime)?;
                reconcile_effects(client, runtime)?;
                Ok("已清空预览模型".into())
            }
            Command::RemoveAsset(id) => {
                let index = runtime
                    .models
                    .iter()
                    .position(|model| model.id == id)
                    .ok_or("模型已移除")?;
                runtime.fx.capture(&runtime.models);
                runtime.models[index].release(client)?;
                runtime.models.remove(index);
                reconcile_skeletons(client, runtime)?;
                reconcile_motions(client, runtime)?;
                reconcile_effects(client, runtime)?;
                Ok("已移除所选模型".into())
            }
            Command::ModelVisible { id, visible } => {
                let model = runtime
                    .models
                    .iter_mut()
                    .find(|model| model.id == id)
                    .ok_or("模型已移除")?;
                if visible && model.asset.is_none() {
                    return Err("模型未加载成功，请查看该项错误或重新加载".into());
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
                skeleton,
                node,
                source,
            } => {
                edit_skeleton_binding(runtime, skeleton, Some((node, source)))?;
                Ok(match source {
                    Some(source) => format!("节点 {node} 跟随节点 {source} 的世界姿态"),
                    None => format!("节点 {node} 已恢复原始骨架姿态"),
                })
            }
            Command::ClearBoneBindings(skeleton) => {
                edit_skeleton_binding(runtime, skeleton, None)?;
                Ok("已清除所选骨架的姿态跟随".into())
            }
            Command::FocusAll => {
                focus_all(runtime)?;
                Ok("已聚焦所有可见模型和骨架".into())
            }
            Command::MotionEnabled { id, enabled } => {
                let previous = runtime.motions.clone();
                runtime.next_motion_activation = runtime.next_motion_activation.wrapping_add(1);
                let activation = runtime.next_motion_activation;
                let motion = motion_resource(runtime, id)?;
                motion.enabled = enabled;
                if enabled {
                    motion.activation = activation;
                }
                if let Err(error) = reconcile_motions(client, runtime) {
                    runtime.motions = previous;
                    reconcile_motions(client, runtime)?;
                    return Err(error);
                }
                Ok("已更新动画启用状态".into())
            }
            Command::RemoveMotion(id) => {
                let index = runtime
                    .motions
                    .iter()
                    .position(|motion| motion.id == id)
                    .ok_or("动画已卸载")?;
                let previous = runtime.motions.remove(index);
                if let Err(error) = reconcile_motions(client, runtime) {
                    runtime.motions.insert(index, previous);
                    reconcile_motions(client, runtime)?;
                    return Err(error);
                }
                Ok("已卸载所选动画".into())
            }
            Command::ClearMotions => {
                for motions in runtime
                    .models
                    .iter_mut()
                    .map(|model| &mut model.motions)
                    .chain(
                        runtime
                            .skeletons
                            .iter_mut()
                            .map(|skeleton| &mut skeleton.motions),
                    )
                {
                    release_motions(client, motions)?;
                }
                runtime.motions.clear();
                Ok("已清空动画".into())
            }
            Command::Playing(playing) => {
                runtime.snapshot.playing = playing;
                runtime.last_frame = None;
                Ok(if playing {
                    "所有动画与特效播放中"
                } else {
                    "所有动画与特效已暂停"
                }
                .into())
            }
            Command::PlaybackSpeed(speed) => {
                if !speed.is_finite() || !(0.1..=4.0).contains(&speed) {
                    return Err("预览速度超出范围".into());
                }
                runtime.snapshot.playback_speed = speed;
                runtime.last_frame = None;
                Ok(format!("动画与特效速度：{speed}×"))
            }
            Command::Seek { track, frame } => {
                match track {
                    PlaybackTrack::Motion(id) => motion_resource(runtime, id)?.seek(frame)?,
                    PlaybackTrack::Effect { binding, slot } => {
                        runtime.fx.seek(&mut runtime.models, binding, slot, frame)?;
                    }
                }
                runtime.snapshot.playing = false;
                runtime.last_frame = None;
                Ok(format!("已定位所选轨道到第 {frame:.2} 步"))
            }
            Command::Step { track, delta } => {
                match track {
                    PlaybackTrack::Motion(id) => {
                        let motion = motion_resource(runtime, id)?;
                        motion.seek((motion.frame + f32::from(delta)).clamp(0.0, motion.frames))?;
                    }
                    PlaybackTrack::Effect { binding, slot } => {
                        runtime.fx.step(&mut runtime.models, binding, slot, delta)?;
                    }
                }
                runtime.snapshot.playing = false;
                runtime.last_frame = None;
                Ok("已逐步调整所选轨道".into())
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
            Command::FocusBone { skeleton, node } => {
                if let Some(node) = node {
                    let resource = runtime
                        .skeletons
                        .iter()
                        .find(|value| value.id == skeleton)
                        .ok_or("骨架已卸载")?;
                    if !resource.bones.iter().any(|bone| bone.index == node) {
                        return Err("骨骼编号无效".into());
                    }
                    runtime.focus_bone = Some((skeleton, node));
                } else {
                    focus_skeleton(runtime, skeleton)?;
                }
                Ok("已更新镜头焦点".into())
            }
            Command::Pan(offset) => {
                let target = camera_target(runtime);
                let center = std::array::from_fn(|axis| target[axis] + offset[axis]);
                if offset.iter().chain(&center).any(|value| !value.is_finite()) {
                    return Err("镜头平移参数无效".into());
                }
                runtime.center = center;
                // Continue from the followed bone's current position, then let
                // the new orbit center remain independent of its animation.
                runtime.focus_bone = None;
                Ok("已平移预览镜头".into())
            }
            Command::Exit | Command::ToggleFullscreen => unreachable!(),
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
                let Runtime { fx, models, .. } = &mut *runtime;
                fx.clear(models);
                let _ = clear_models(state.client, &mut runtime);
                let _ = clear_skeletons(state.client, &mut runtime);
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
    fn directional(light: crate::preview::lighting::DirectionalLight) -> Self {
        let [r, g, b] = light.color;
        Self {
            unknown_00: 0,
            diffuse: [r, g, b, 1.0],
            specular: [0.0; 4],
            ambient: [0.0; 4],
            direction: light.direction,
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

fn camera_target(runtime: &Runtime) -> [f32; 3] {
    runtime
        .focus_bone
        .and_then(|(skeleton, node)| {
            runtime
                .skeletons
                .iter()
                .find(|value| value.id == skeleton)
                .and_then(|skeleton| skeleton.bones.iter().find(|bone| bone.index == node))
        })
        .map_or(runtime.center, |bone| bone.position)
}

unsafe fn render_frame(state: &State, runtime: &mut Runtime) -> Result<(), String> {
    let requested = state.control.viewport();
    let options = state.control.preview_options();
    runtime.snapshot.viewport = requested;
    let pointer = unsafe { state.client.read::<*mut c_void>(0x1e811a3c) };
    let device =
        unsafe { IDirect3DDevice9::from_raw_borrowed(&pointer) }.ok_or("原生绘制设备尚未初始化")?;
    let mut viewport = unsafe {
        viewport::NativeViewport::begin(device, state.client, requested, options.background_color)
    }?;
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
    if runtime.snapshot.playing {
        let speed = runtime.snapshot.playback_speed;
        for model in &mut runtime.models {
            if !model.motions.is_empty() || !model.effects.bindings.is_empty() {
                model.frame = crate::preview::advance_frame(model.frame, elapsed, speed);
            }
        }
        for motion in &mut runtime.motions {
            if motion.enabled
                && runtime
                    .models
                    .iter()
                    .map(|model| &model.motions)
                    .chain(runtime.skeletons.iter().map(|skeleton| &skeleton.motions))
                    .any(|motions| motions.iter().any(|bound| bound.resource == motion.id))
            {
                motion.frame = crate::preview::looping_motion_frame(
                    crate::preview::advance_frame(motion.frame, elapsed, speed),
                    motion.frames,
                );
            }
        }
    }
    let snapshot = &runtime.snapshot;
    let target = camera_target(runtime);
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
        let lighting = options.lighting_preset.lighting();
        let [r, g, b] = lighting.ambient;
        parameter(u32::from_be_bytes([255, r, g, b]) as usize, 14);
        for (slot, light) in lighting.lights.into_iter().enumerate() {
            let light = Light::directional(light);
            parameter(&light as *const Light as usize, 90 + slot as u32);
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
        for model in &mut runtime.models {
            let motion_frames = motion_frames(&model.motions, &runtime.motions);
            let mut effects = model.effects.sample(model.frame);
            if let Some(asset) = &mut model.asset {
                let sampled = if model.visible {
                    asset.draw(state.client, &WORLD, &motion_frames, Some(&mut effects))
                } else {
                    asset.update_skeleton(state.client, &motion_frames, &WORLD)
                };
                if let Err(error) = sampled {
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
            model.effects.snapshot = Arc::new(effects.bindings);
        }
        for skeleton in &mut runtime.skeletons {
            if let Some(native) = &mut skeleton.native {
                let frames = motion_frames(&skeleton.motions, &runtime.motions);
                match native.update(state.client, &frames, &WORLD) {
                    Ok(()) => {
                        skeleton.bones = Arc::new(native.bones());
                        skeleton.error = None;
                    }
                    Err(error) => skeleton.error = Some(error.into()),
                }
            } else if let Some(model) = runtime
                .models
                .iter()
                .find(|model| model.asset.is_some() && model.uses_skeleton(&skeleton.source))
            {
                skeleton.bones = model.bones.clone();
            }
        }
    }
    unsafe { guides::draw(device, camera, options) }
        .map_err(|error| format!("无法绘制预览网格与坐标轴：{error}"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(resource: u64, model: usize, nodes: &[usize], activation: u64) -> MotionBinding {
        MotionBinding {
            resource,
            skeleton: model as u64,
            target: MotionTarget::Model(model as u64),
            nodes: nodes.to_vec(),
            activation,
        }
    }

    #[test]
    fn independent_motion_groups_share_a_model_and_only_overlaps_stop() {
        let (selected, stopped) = select_motion_bindings(vec![
            candidate(1, 0, &[100, 200], 1),
            candidate(2, 0, &[300, 400], 2),
            candidate(3, 1, &[100, 200], 3),
            candidate(4, 0, &[200], 4),
        ]);
        assert_eq!(
            selected
                .iter()
                .map(|binding| binding.resource)
                .collect::<Vec<_>>(),
            [4, 3, 2]
        );
        assert_eq!(
            stopped,
            [1],
            "the entire older clip stops, including node 100"
        );

        let (selected, stopped) = select_motion_bindings(vec![
            candidate(1, 0, &[100, 200], 5),
            candidate(2, 0, &[300, 400], 2),
            candidate(3, 1, &[100, 200], 3),
            candidate(4, 0, &[200], 4),
        ]);
        assert_eq!(
            stopped,
            [4],
            "reenabling the first clip makes it the newest"
        );
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn standalone_motion_groups_use_the_same_conflict_policy() {
        let mut first = candidate(1, 0, &[100], 1);
        first.target = MotionTarget::Skeleton(7);
        let mut second = candidate(2, 0, &[200], 2);
        second.target = MotionTarget::Skeleton(7);
        let mut replacement = candidate(3, 0, &[100], 3);
        replacement.target = MotionTarget::Skeleton(7);
        let (selected, stopped) = select_motion_bindings(vec![first, second, replacement]);
        assert_eq!(stopped, [1]);
        assert_eq!(
            selected
                .iter()
                .map(|binding| binding.resource)
                .collect::<Vec<_>>(),
            [3, 2]
        );
    }

    #[test]
    fn shared_skeleton_motion_stops_all_model_copies_and_keeps_other_groups() {
        let mut candidates = Vec::new();
        for model in 0..2 {
            let base = model * 1000;
            for (resource, nodes) in [
                (1, vec![base, base + 1]),
                (2, vec![base + 2]),
                (3, vec![base + 1]),
            ] {
                let mut binding = candidate(resource, model, &nodes, resource);
                binding.skeleton = 7;
                candidates.push(binding);
            }
        }
        let (selected, stopped) = select_motion_bindings(candidates);
        assert_eq!(stopped, [1]);
        for model in 0..2 {
            let resources = selected
                .iter()
                .filter(|binding| binding.target == MotionTarget::Model(model))
                .map(|binding| binding.resource)
                .collect::<Vec<_>>();
            assert_eq!(resources, [3, 2]);
        }
    }

    #[test]
    fn skeleton_snapshots_and_focus_do_not_require_an_active_model() {
        let source = ResourceRef::white_texture();
        let mut runtime = Runtime {
            skeletons: [(7, [10.0, 20.0, 30.0]), (8, [100.0, 200.0, 300.0])]
                .into_iter()
                .map(|(id, position)| SkeletonResource {
                    id,
                    source: source.clone(),
                    native: None,
                    motions: Vec::new(),
                    bones: Arc::new(vec![Bone {
                        index: 0,
                        parent: None,
                        position,
                    }]),
                    bindings: Arc::new(vec![None]),
                    error: None,
                })
                .collect(),
            ..Runtime::default()
        };
        focus_skeleton(&mut runtime, 8).unwrap();
        assert_eq!(runtime.center, [100.0, 200.0, 300.0]);
        runtime.focus_bone = Some((7, 0));
        assert_eq!(camera_target(&runtime), [10.0, 20.0, 30.0]);
        let definition = AssetBundle {
            name: "unavailable model".into(),
            model: source,
            skeleton: None,
            textures: Vec::new(),
        };
        runtime.models.push(Model {
            id: 1,
            identity: None,
            definition: definition.clone(),
            bundle: definition,
            asset: None,
            visible: false,
            error: Some("not constructed".into()),
            motions: Vec::new(),
            frame: 0.0,
            bones: Arc::default(),
            effects: Effects::default(),
        });
        unsafe { clear_models(Client { base: 0 }, &mut runtime) }.unwrap();
        assert_eq!(runtime.focus_bone, Some((7, 0)));
        assert_eq!(camera_target(&runtime), [10.0, 20.0, 30.0]);
        refresh_snapshot(&mut runtime);
        assert_eq!(runtime.snapshot.skeletons.len(), 2);
        assert_eq!(
            runtime.snapshot.skeletons[1].bone_bindings.as_ref(),
            &[None]
        );
        assert!(runtime.snapshot.models.is_empty());
    }

    #[test]
    fn each_loaded_motion_retains_and_seeks_its_own_cursor_without_an_active_model() {
        let source = ResourceRef::white_texture();
        let mut runtime = Runtime {
            motions: vec![
                MotionResource {
                    id: 11,
                    source: source.clone(),
                    enabled: false,
                    frames: 30.0,
                    frame: 7.0,
                    activation: 1,
                },
                MotionResource {
                    id: 22,
                    source,
                    enabled: true,
                    frames: 90.0,
                    frame: 12.0,
                    activation: 2,
                },
            ],
            ..Runtime::default()
        };
        motion_resource(&mut runtime, 11)
            .unwrap()
            .seek(25.0)
            .unwrap();
        assert!(
            motion_resource(&mut runtime, 11)
                .unwrap()
                .seek(31.0)
                .is_err()
        );
        assert!(motion_resource(&mut runtime, 99).is_err());
        refresh_snapshot(&mut runtime);
        assert_eq!(runtime.snapshot.motions[0].frame, Some(25.0));
        assert_eq!(runtime.snapshot.motions[1].frame, Some(12.0));
        assert!(!runtime.snapshot.motions[0].enabled);
        assert!(
            runtime
                .snapshot
                .motions
                .iter()
                .all(|motion| motion.skeleton.is_none())
        );
    }
}
