//! Commands and owned snapshots cross the render/game thread boundary.

use crate::inspect::{Document, Kind};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

pub(crate) mod effects;

pub(crate) const DEFAULT_BACKGROUND_COLOR: [u8; 3] = [16, 19, 22];

pub(crate) fn advance_frame(frame: f32, elapsed: f32, speed: f32) -> f32 {
    frame + elapsed.clamp(0.0, 0.1) * 30.0 * speed
}

pub(crate) fn looping_motion_frame(frame: f32, frames: f32) -> f32 {
    if frames <= 0.0 {
        0.0
    } else if frame > frames {
        frame.rem_euclid(frames)
    } else {
        frame
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PreviewOptions {
    pub background_color: [u8; 3],
    pub show_grid: bool,
    pub show_axes: bool,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            background_color: DEFAULT_BACKGROUND_COLOR,
            show_grid: true,
            show_axes: true,
        }
    }
}

/// An identified resource keeps its decoded source bytes alive independently
/// of the file currently selected in the browser.
#[derive(Clone)]
pub(crate) struct ResourceRef {
    pub document: Arc<Document>,
    pub node: usize,
}

impl ResourceRef {
    pub fn bytes(&self) -> Result<&[u8], String> {
        self.document
            .bytes(resource_node(&self.document, self.node)?)
            .ok_or_else(|| "资源节点失效".into())
    }

    pub fn name(&self) -> String {
        format!(
            "{} · {}",
            self.document.nodes[self.document.root].name, self.document.nodes[self.node].name
        )
    }

    pub fn short_name(&self) -> String {
        let root = &self.document.nodes[self.document.root].name;
        format!(
            "{} · {}",
            root.rsplit(['/', '\\']).next().unwrap_or(root),
            self.document.nodes[self.node].name
        )
    }

    pub fn same_source(&self, other: &Self) -> bool {
        let (Ok(first), Ok(second)) = (
            resource_node(&self.document, self.node),
            resource_node(&other.document, other.node),
        ) else {
            return false;
        };
        let a = &self.document.nodes[first];
        let b = &other.document.nodes[second];
        first == second
            && a.kind == b.kind
            && a.range == b.range
            && Arc::ptr_eq(
                &self.document.buffers[a.buffer],
                &other.document.buffers[b.buffer],
            )
    }

    pub fn kind(&self) -> Kind {
        resource_node(&self.document, self.node)
            .map_or(Kind::Unknown, |node| self.document.nodes[node].kind)
    }
}

/// The inspection document retains every encoded layer. Runtime resource
/// handles transparently read its payload, while preserving any layer error.
fn resource_node(document: &Document, node: usize) -> Result<usize, String> {
    let payload = document
        .payload(node)
        .ok_or("资源包装层未成功解码，或引用链无效")?;
    let mut current = node;
    loop {
        let value = &document.nodes[current];
        if let Some(error) = &value.error {
            return Err(error.clone());
        }
        if current == payload {
            return Ok(payload);
        }
        // payload() already checked this entire single-child chain for valid
        // indices and cycles. Revisit it only to preserve intermediate errors.
        current = value.children[0];
    }
}

#[derive(Clone)]
pub(crate) struct AssetBundle {
    pub model: ResourceRef,
    pub skeleton: Option<ResourceRef>,
    pub textures: Vec<ResourceRef>,
    pub name: String,
}

impl AssetBundle {
    pub fn same_source(&self, other: &Self) -> bool {
        self.model.same_source(&other.model)
            && self.textures.len() == other.textures.len()
            && self
                .textures
                .iter()
                .zip(&other.textures)
                .all(|(a, b)| a.same_source(b))
            && match (&self.skeleton, &other.skeleton) {
                (Some(a), Some(b)) => a.same_source(b),
                (None, None) => true,
                _ => false,
            }
    }

    /// Containers index the bundles found in their own branches.
    /// Reference targets remain owned by their original branch; the referring
    /// package gets the combination selected by its own member descriptors.
    pub fn find_with_nodes(document: Arc<Document>) -> (Vec<Self>, Vec<Vec<usize>>) {
        fn payload(document: &Document, index: usize) -> usize {
            resource_node(document, index).unwrap_or(index)
        }
        let mut parents = vec![None; document.nodes.len()];
        for (parent, node) in document.nodes.iter().enumerate() {
            if node.kind != Kind::StageResourceReference {
                for &child in &node.children {
                    parents[child] = Some(parent);
                }
            }
        }
        let mut by_node = vec![Vec::new(); document.nodes.len()];
        let mut ordinals = HashMap::<usize, usize>::new();
        let mut result: Vec<Self> = Vec::new();
        let mut add = |owner: usize, model: usize, skeleton: Option<usize>, textures: usize| {
            let texture = &document.nodes[textures];
            let valid_texture = match texture.kind {
                Kind::Png | Kind::Dds => true,
                Kind::Txb | Kind::Archive => {
                    (texture.kind == Kind::Txb || !texture.children.is_empty())
                        && texture.children.iter().all(|&index| {
                            let image = &document.nodes[payload(&document, index)];
                            matches!(image.kind, Kind::Png | Kind::Dds) || image.range.is_empty()
                        })
                }
                _ => false,
            };
            if document.nodes[model].kind != Kind::Fmod
                || skeleton.is_some_and(|index| document.nodes[index].kind != Kind::Fskl)
                || !valid_texture
                || [Some(model), skeleton, Some(textures)]
                    .into_iter()
                    .flatten()
                    .any(|index| document.nodes[index].error.is_some())
            {
                return;
            }
            let resource = |node| ResourceRef {
                document: document.clone(),
                node,
            };
            let mut bundle = Self {
                model: resource(model),
                skeleton: skeleton.map(resource),
                textures: vec![resource(textures)],
                name: String::new(),
            };
            // References resolve to the existing target node. A second path to
            // the same complete bundle is one preview; distinct original alias
            // entries still retain different node identities in same_source.
            let bundle_index = match result
                .iter()
                .position(|existing| existing.same_source(&bundle))
            {
                Some(index) => index,
                None => {
                    let mut named = document.root;
                    let mut current = owner;
                    while let Some(parent) = parents[current] {
                        if document.nodes[parent].kind == Kind::Mha {
                            named = current;
                            break;
                        }
                        current = parent;
                    }
                    let ordinal = ordinals.entry(named).or_default();
                    *ordinal += 1;
                    bundle.name = if named == document.root {
                        format!("{} · 模型 {ordinal}", document.nodes[named].name)
                    } else {
                        format!(
                            "{} / {} · 模型 {ordinal}",
                            document.nodes[document.root].name, document.nodes[named].name
                        )
                    };
                    let index = result.len();
                    result.push(bundle);
                    index
                }
            };
            let mut current = Some(owner);
            while let Some(index) = current {
                if by_node[index].contains(&bundle_index) {
                    break;
                }
                by_node[index].push(bundle_index);
                current = parents[index];
            }
        };
        for (node_index, node) in document.nodes.iter().enumerate() {
            if node.kind == Kind::StageObjectPackage {
                if node.error.is_some() {
                    continue;
                }
                let Some(bytes) = document.bytes(node_index) else {
                    continue;
                };
                let Ok(package) =
                    mhf_resource::stage::ObjectPackage::parse(bytes, node.children.len())
                else {
                    continue;
                };
                let member_node = |kind| {
                    package
                        .member(kind)
                        .and_then(|member| node.children.get(member.entry.index).copied())
                };
                let Some(texture_member) = member_node(3) else {
                    continue;
                };
                let Ok(textures) = resource_node(&document, texture_member) else {
                    continue;
                };
                let skeleton = match member_node(2) {
                    Some(index) => {
                        let Ok(index) = resource_node(&document, index) else {
                            continue;
                        };
                        (document.nodes[index].kind != Kind::Empty).then_some(index)
                    }
                    None => None,
                };
                for member in package.members.iter().filter(|member| member.kind == 1) {
                    let Some(&model_member) = node.children.get(member.entry.index) else {
                        continue;
                    };
                    if let Ok(model) = resource_node(&document, model_member) {
                        add(node_index, model, skeleton, textures);
                    }
                }
                continue;
            }
            if !matches!(
                node.kind,
                Kind::Archive | Kind::Momo | Kind::Mha | Kind::Stage
            ) {
                continue;
            }
            let children: Vec<_> = node
                .children
                .iter()
                .map(|&index| payload(&document, index))
                .collect();
            for (position, &member) in children.iter().enumerate() {
                let member_node = &document.nodes[member];
                let (group, model, skeleton, texture_position) = if member_node.kind == Kind::Fmod {
                    // Some directories store parallel model, skeleton, and
                    // texture lists instead of interleaving complete bundles.
                    let start = children[..position]
                        .iter()
                        .rposition(|&index| document.nodes[index].kind != Kind::Fmod)
                        .map_or(0, |index| index + 1);
                    let count = children[start..]
                        .iter()
                        .take_while(|&&index| document.nodes[index].kind == Kind::Fmod)
                        .count();
                    let columns = count > 1
                        && children.get(start + count..start + 3 * count).is_some_and(
                            |remaining| {
                                remaining[..count]
                                    .iter()
                                    .all(|&index| document.nodes[index].kind == Kind::Fskl)
                                    && remaining[count..].iter().all(|&index| {
                                        matches!(
                                            document.nodes[index].kind,
                                            Kind::Txb | Kind::Png | Kind::Dds
                                        )
                                    })
                            },
                        );
                    if columns {
                        (
                            node_index,
                            member,
                            Some(children[position + count]),
                            position + 2 * count,
                        )
                    } else {
                        let Some(&next) = children.get(position + 1) else {
                            continue;
                        };
                        let skeleton = (document.nodes[next].kind == Kind::Fskl).then_some(next);
                        (
                            node_index,
                            member,
                            skeleton,
                            position + 1 + usize::from(skeleton.is_some()),
                        )
                    }
                } else if matches!(member_node.kind, Kind::Archive | Kind::Momo | Kind::Mha)
                    && member_node.error.is_none()
                    && member_node.children.len() == 2
                {
                    // A model package can keep geometry + skeleton together in
                    // an inner directory, with its texture bank beside it.
                    let model = payload(&document, member_node.children[0]);
                    let skeleton = payload(&document, member_node.children[1]);
                    if document.nodes[model].kind != Kind::Fmod
                        || document.nodes[skeleton].kind != Kind::Fskl
                    {
                        continue;
                    }
                    (member, model, Some(skeleton), position + 1)
                } else {
                    continue;
                };
                let Some(&textures) = children.get(texture_position) else {
                    continue;
                };
                add(group, model, skeleton, textures);
            }
        }
        (result, by_node)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Bone {
    pub index: usize,
    pub parent: Option<usize>,
    pub position: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Camera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_y: f32,
    pub aspect: f32,
}

/// A rectangle relative to the complete UI content area, independent of DPI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

impl Viewport {
    pub fn clipped(self) -> Self {
        if [self.x, self.y, self.width, self.height]
            .iter()
            .any(|value| !value.is_finite())
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return Self {
                width: 0.0,
                height: 0.0,
                ..Self::default()
            };
        }
        // Intersect the original edges; clamping the origin before adding the
        // width would incorrectly enlarge a rectangle extending offscreen.
        let left = f64::from(self.x).clamp(0.0, 1.0);
        let top = f64::from(self.y).clamp(0.0, 1.0);
        let right = (f64::from(self.x) + f64::from(self.width)).clamp(0.0, 1.0);
        let bottom = (f64::from(self.y) + f64::from(self.height)).clamp(0.0, 1.0);
        Self {
            x: left as f32,
            y: top as f32,
            width: (right - left).max(0.0) as f32,
            height: (bottom - top).max(0.0) as f32,
        }
    }

    pub fn pixels(self, target_width: u32, target_height: u32) -> Option<ViewportPixels> {
        if target_width == 0 || target_height == 0 {
            return None;
        }
        let rect = self.clipped();
        let edge = |value: f64, size: u32| (value.clamp(0.0, 1.0) * f64::from(size)).round() as u32;
        let x = edge(f64::from(rect.x), target_width);
        let y = edge(f64::from(rect.y), target_height);
        let right = edge(f64::from(rect.x) + f64::from(rect.width), target_width);
        let bottom = edge(f64::from(rect.y) + f64::from(rect.height), target_height);
        (right > x && bottom > y).then_some(ViewportPixels {
            x,
            y,
            width: right.saturating_sub(x),
            height: bottom.saturating_sub(y),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewportPixels {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ViewportPixels {
    pub fn aspect(self) -> f32 {
        self.width as f32 / self.height as f32
    }

    pub fn normalized(self, target_width: u32, target_height: u32) -> Viewport {
        Viewport {
            x: self.x as f32 / target_width as f32,
            y: self.y as f32 / target_height as f32,
            width: self.width as f32 / target_width as f32,
            height: self.height as f32 / target_height as f32,
        }
    }
}

impl Camera {
    pub fn distance(self) -> f32 {
        self.eye
            .into_iter()
            .zip(self.target)
            .map(|(eye, target)| (eye - target).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Translate the camera and orbit center so points on the focus plane follow
    /// the drag. Delta is a fraction of viewport width/height, independent of DPI.
    pub fn pan_offset(self, delta: [f32; 2]) -> Option<[f32; 3]> {
        let scale = 2.0 * self.distance() * (self.fov_y * 0.5).tan();
        if !scale.is_finite() || scale <= 0.0 || self.aspect <= 0.0 {
            return None;
        }
        let (view, _) = self.matrices(1.0, 200_000.0);
        let offset = std::array::from_fn(|axis| {
            scale * (-view[axis * 4] * delta[0] * self.aspect + view[axis * 4 + 1] * delta[1])
        });
        offset
            .iter()
            .all(|value| value.is_finite())
            .then_some(offset)
    }

    /// Row-major D3DX look-at and perspective matrices used by the native shader slots.
    pub fn matrices(self, near: f32, far: f32) -> ([f32; 16], [f32; 16]) {
        fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
            a.into_iter().zip(b).map(|(a, b)| a * b).sum()
        }
        fn normalize(v: [f32; 3]) -> [f32; 3] {
            let n = dot(v, v).sqrt();
            v.map(|v| v / n)
        }
        fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        }
        let z = normalize(std::array::from_fn(|i| self.eye[i] - self.target[i]));
        let x = normalize(cross(self.up, z));
        let y = cross(z, x);
        let view = [
            x[0],
            y[0],
            z[0],
            0.0,
            x[1],
            y[1],
            z[1],
            0.0,
            x[2],
            y[2],
            z[2],
            0.0,
            -dot(x, self.eye),
            -dot(y, self.eye),
            -dot(z, self.eye),
            1.0,
        ];
        let scale = 1.0 / (self.fov_y * 0.5).tan();
        let projection = [
            scale / self.aspect,
            0.0,
            0.0,
            0.0,
            0.0,
            scale,
            0.0,
            0.0,
            0.0,
            0.0,
            far / (near - far),
            -1.0,
            0.0,
            0.0,
            near * far / (near - far),
            0.0,
        ];
        (view, projection)
    }

    /// Match the native right-handed perspective camera, returning normalized
    /// screen coordinates so the overlay's DPI does not affect bone positions.
    pub fn project(self, position: [f32; 3]) -> Option<[f32; 2]> {
        fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
            a.into_iter().zip(b).map(|(a, b)| a * b).sum()
        }
        fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        }
        fn normalize(value: [f32; 3]) -> Option<[f32; 3]> {
            let length = dot(value, value).sqrt();
            (length.is_finite() && length > f32::EPSILON).then(|| value.map(|v| v / length))
        }
        if !self.fov_y.is_finite()
            || !(0.0..std::f32::consts::PI).contains(&self.fov_y)
            || !self.aspect.is_finite()
            || self.aspect <= 0.0
        {
            return None;
        }
        let forward = normalize(std::array::from_fn(|axis| {
            self.target[axis] - self.eye[axis]
        }))?;
        let right = normalize(cross(forward, self.up))?;
        let up = cross(right, forward);
        let relative = std::array::from_fn(|axis| position[axis] - self.eye[axis]);
        let depth = dot(relative, forward);
        if !depth.is_finite() || depth <= 0.001 {
            return None;
        }
        let vertical = depth * (self.fov_y / 2.0).tan();
        let point = [
            0.5 + 0.5 * dot(relative, right) / (vertical * self.aspect),
            0.5 - 0.5 * dot(relative, up) / vertical,
        ];
        point.iter().all(|value| value.is_finite()).then_some(point)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LoadedMesh {
    /// Stable source Object ordinal, including hidden meshes.
    pub index: usize,
    pub vertices: usize,
    pub visible: bool,
}

#[derive(Clone)]
pub(crate) struct LoadedModel {
    pub id: u64,
    pub name: Arc<str>,
    pub visible: bool,
    pub error: Option<Arc<str>>,
    pub meshes: Arc<Vec<LoadedMesh>>,
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub ready: bool,
    pub playing: bool,
    pub playback_speed: f32,
    pub bones: Arc<Vec<Bone>>,
    /// Optional source node per target node, scoped to the active model.
    pub bone_bindings: Arc<Vec<Option<usize>>>,
    pub camera: Option<Camera>,
    /// Actual pixel-rounded rectangle from the same rendered frame as camera.
    pub viewport: Viewport,
    pub distance: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub message: Arc<str>,
    pub models: Arc<Vec<LoadedModel>>,
    pub active_model: Option<u64>,
    pub scene: Option<Arc<str>>,
    pub scene_visible: bool,
    pub motion: Option<Arc<str>>,
    pub motion_frames: f32,
    pub motion_frame: f32,
    pub effects: Arc<Vec<effects::BindingSnapshot>>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            ready: false,
            playing: true,
            playback_speed: 1.0,
            bones: Arc::new(Vec::new()),
            bone_bindings: Arc::default(),
            camera: None,
            viewport: Viewport::default(),
            distance: 350.0,
            pitch: 10.0,
            yaw: 0.0,
            message: "正在初始化资源工作台".into(),
            models: Arc::new(Vec::new()),
            active_model: None,
            scene: None,
            scene_visible: false,
            motion: None,
            motion_frames: 0.0,
            motion_frame: 0.0,
            effects: Arc::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaybackTrack {
    Motion,
    Effect { binding: u64, slot: usize },
}

pub(crate) enum Command {
    LoadAssets(Vec<AssetBundle>),
    AddAsset(AssetBundle),
    TriggerEffect(ResourceRef),
    TriggerEffectDefinition {
        model: u64,
        binding: u64,
        slot: usize,
    },
    StopEffectDefinition {
        model: u64,
        binding: u64,
        slot: usize,
    },
    RemoveEffectDefinition {
        model: u64,
        binding: u64,
        slot: usize,
    },
    RemoveAsset(u64),
    ClearAssets,
    SelectModel(u64),
    ModelVisible {
        id: u64,
        visible: bool,
    },
    MeshVisible {
        model: u64,
        mesh: usize,
        visible: bool,
    },
    IsolateMesh {
        model: u64,
        mesh: usize,
    },
    ShowAllMeshes(u64),
    BoneBinding {
        model: u64,
        node: usize,
        source: Option<usize>,
    },
    ClearBoneBindings(u64),
    FocusAll,
    LoadScene(AssetBundle),
    UnloadScene,
    SceneVisible(bool),
    LoadMotion(ResourceRef),
    UnloadMotion,
    Playing(bool),
    PlaybackSpeed(f32),
    Seek {
        track: PlaybackTrack,
        frame: f32,
    },
    Step {
        track: PlaybackTrack,
        delta: i8,
    },
    Camera {
        distance: f32,
        pitch: f32,
        yaw: f32,
    },
    Pan([f32; 3]),
    FocusBone(Option<usize>),
    ToggleFullscreen,
    Exit,
}

#[derive(Default)]
struct Shared {
    snapshot: Snapshot,
    commands: Vec<Command>,
    viewport: Viewport,
    preview_options: PreviewOptions,
    closing: bool,
}

#[derive(Default)]
pub(crate) struct Control {
    shared: Mutex<Shared>,
}

impl Control {
    pub fn set_preview_options(&self, options: PreviewOptions) {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .preview_options = options;
    }

    pub fn preview_options(&self) -> PreviewOptions {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .preview_options
    }

    pub fn set_viewport(&self, viewport: Viewport) {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .viewport = viewport.clipped();
    }

    pub fn viewport(&self) -> Viewport {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .viewport
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot
            .clone()
    }

    pub fn closing(&self) -> bool {
        self.shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closing
    }

    pub fn send(&self, command: Command) -> Result<(), String> {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(command, Command::Exit) {
            // Closing the window must work even while resource requests fill
            // the queue. Pending loads need not run before native cleanup.
            if !shared.closing {
                shared.commands.clear();
                shared.commands.push(command);
                shared.closing = true;
            }
            return Ok(());
        }
        if shared.closing {
            return Err("工作台正在结束".into());
        }
        // Pan is a relative motion: add consecutive deltas instead of dropping
        // earlier input while the native thread is still drawing/loading.
        if let Some(Command::Pan(pending)) = shared.commands.last_mut()
            && let Command::Pan(delta) = &command
        {
            for axis in 0..3 {
                pending[axis] += delta[axis];
            }
            return Ok(());
        }
        // Coalesce consecutive slider updates without moving a seek across an
        // resource switch, whose ordering changes its meaning.
        if let Some(last) = shared.commands.last_mut()
            && match (&*last, &command) {
                (Command::Seek { track: first, .. }, Command::Seek { track: second, .. }) => {
                    first == second
                }
                (Command::PlaybackSpeed(_), Command::PlaybackSpeed(_))
                | (Command::Camera { .. }, Command::Camera { .. })
                | (Command::LoadAssets(_), Command::LoadAssets(_)) => true,
                _ => false,
            }
        {
            *last = command;
            return Ok(());
        }
        if shared.commands.len() >= 32 {
            return Err("请等待原生模型操作完成".into());
        }
        shared.commands.push(command);
        Ok(())
    }

    pub fn commands(&self) -> Vec<Command> {
        std::mem::take(
            &mut self
                .shared
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .commands,
        )
    }

    pub fn publish(&self, snapshot: Snapshot) {
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
    fn viewport_rounds_edges_and_reports_the_actual_render_rectangle() {
        assert_eq!(
            Viewport::default().pixels(1920, 1080),
            Some(ViewportPixels {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            })
        );
        let requested = Viewport {
            x: 0.25,
            y: 0.25,
            width: 0.5,
            height: 0.5,
        };
        let pixels = requested.pixels(101, 99).unwrap();
        assert_eq!(
            pixels,
            ViewportPixels {
                x: 25,
                y: 25,
                width: 51,
                height: 49,
            }
        );
        let actual = pixels.normalized(101, 99);
        assert_ne!(actual, requested);
        assert_eq!(actual.pixels(101, 99), Some(pixels));
        assert_eq!(pixels.aspect(), 51.0 / 49.0);
    }

    #[test]
    fn viewport_intersection_does_not_enlarge_partially_offscreen_regions() {
        let left = Viewport {
            x: -0.2,
            y: 0.2,
            width: 0.5,
            height: 0.4,
        };
        assert_eq!(
            left.pixels(1000, 500),
            Some(ViewportPixels {
                x: 0,
                y: 100,
                width: 300,
                height: 200,
            })
        );
        let bottom_right = Viewport {
            x: 0.9,
            y: 0.9,
            width: 0.5,
            height: 0.5,
        };
        assert_eq!(
            bottom_right.pixels(1000, 500),
            Some(ViewportPixels {
                x: 900,
                y: 450,
                width: 100,
                height: 50,
            })
        );
        for empty in [
            Viewport { x: 1.1, ..left },
            Viewport {
                width: -1.0,
                ..left
            },
            Viewport {
                height: 0.0,
                ..left
            },
            Viewport {
                x: f32::NAN,
                ..left
            },
            Viewport {
                y: f32::INFINITY,
                ..left
            },
            Viewport {
                width: f32::INFINITY,
                ..left
            },
            Viewport {
                height: f32::NAN,
                ..left
            },
            Viewport {
                x: 0.0,
                width: 0.0001,
                ..left
            },
        ] {
            assert_eq!(empty.pixels(1000, 500), None, "{empty:?}");
        }
        assert_eq!(Viewport::default().pixels(0, 500), None);
        assert_eq!(Viewport::default().pixels(1000, 0), None);
    }

    #[test]
    fn viewport_projection_scales_with_the_render_target_not_ui_dpi() {
        let viewport = Viewport {
            x: 0.2,
            y: 0.1,
            width: 0.6,
            height: 0.8,
        };
        // The same content-area ratios at 100%, 125%, 150%, and 200% scale.
        for (width, height) in [(800, 600), (1000, 750), (1200, 900), (1600, 1200)] {
            let pixels = viewport.pixels(width, height).unwrap();
            let camera = Camera {
                eye: [0.0, 0.0, 10.0],
                target: [0.0; 3],
                up: [0.0, 1.0, 0.0],
                fov_y: std::f32::consts::FRAC_PI_2,
                aspect: pixels.aspect(),
            };
            let point = camera.project([5.0, 0.0, 0.0]).unwrap();
            let actual_x = pixels.x as f32 + point[0] * pixels.width as f32;
            let center_x = pixels.x as f32 + pixels.width as f32 / 2.0;
            // Square projection scale: a horizontal displacement of half the
            // depth occupies one quarter of the actual viewport's height.
            assert!((actual_x - center_x - pixels.height as f32 / 4.0).abs() < 0.001);
            let actual = pixels.normalized(width, height);
            let overlay_x = (actual.x + point[0] * actual.width) * width as f32;
            assert!((overlay_x - actual_x).abs() < 0.001);
        }
    }

    #[test]
    fn viewport_updates_are_independent_of_commands_and_rendered_snapshots() {
        let control = Control::default();
        assert_eq!(control.viewport(), Viewport::default());
        for _ in 0..32 {
            control.send(Command::Playing(true)).unwrap();
        }
        let mut requested = Viewport::default();
        for i in 0..1000 {
            requested.x = i as f32 / 2000.0;
            requested.width = 0.5;
            control.set_viewport(requested);
        }
        let actual = Viewport {
            x: 0.25,
            width: 0.5,
            ..Viewport::default()
        };
        control.publish(Snapshot {
            viewport: actual,
            ..Snapshot::default()
        });
        assert_eq!(control.viewport(), requested.clipped());
        assert_eq!(control.snapshot().viewport, actual);
        assert_eq!(control.commands().len(), 32);
        control.set_viewport(Viewport::default());
        assert_eq!(control.viewport(), Viewport::default());
    }

    #[test]
    fn bone_projection_matches_native_perspective_and_ignores_points_behind_eye() {
        let camera = Camera {
            eye: [0.0, 0.0, 10.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            fov_y: std::f32::consts::FRAC_PI_2,
            aspect: 2.0,
        };
        assert_eq!(camera.project([0.0; 3]), Some([0.5, 0.5]));
        assert_eq!(camera.project([5.0, 5.0, 0.0]), Some([0.625, 0.25]));
        assert_eq!(camera.project([0.0, 0.0, 20.0]), None);
    }
    #[test]
    fn camera_matrices_use_native_right_handed_depth() {
        let camera = Camera {
            eye: [0.0, 0.0, 10.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            fov_y: std::f32::consts::FRAC_PI_2,
            aspect: 2.0,
        };
        let (view, projection) = camera.matrices(1.0, 100.0);
        assert_eq!([view[12], view[13], view[14]], [0.0, 0.0, -10.0]);
        for (z, expected) in [(-1.0, 0.0), (-100.0, 1.0)] {
            let depth = (z * projection[10] + projection[14]) / (z * projection[11]);
            assert!((depth - expected).abs() < 0.00001);
        }
    }

    #[test]
    fn panning_keeps_focus_plane_motion_under_the_pointer_at_any_zoom_or_angle() {
        for distance in [1.0, 350.0, 100_000.0] {
            for aspect in [0.75, 2.0] {
                for direction in [[0.0, 0.0, 1.0], [0.6, 0.4, -0.7]] {
                    let camera = Camera {
                        eye: direction.map(|value| value * distance),
                        target: [0.0; 3],
                        up: [0.0, 1.0, 0.0],
                        fov_y: std::f32::consts::FRAC_PI_3,
                        aspect,
                    };
                    let delta = [0.08, -0.05];
                    let offset = camera.pan_offset(delta).unwrap();
                    let moved = Camera {
                        eye: std::array::from_fn(|axis| camera.eye[axis] + offset[axis]),
                        target: offset,
                        ..camera
                    };
                    let point = moved.project(camera.target).unwrap();
                    for axis in 0..2 {
                        assert!((point[axis] - (0.5 + delta[axis])).abs() < 0.00001);
                    }
                    assert!(camera.pan_offset([f32::NAN, 0.0]).is_none());
                }
            }
        }
    }

    #[test]
    fn queued_pan_deltas_accumulate_without_crossing_focus_changes() {
        let control = Control::default();
        for _ in 0..100 {
            control.send(Command::Pan([1.0, -2.0, 3.0])).unwrap();
        }
        control.send(Command::FocusAll).unwrap();
        control.send(Command::Pan([4.0, 5.0, 6.0])).unwrap();
        assert!(matches!(
            control.commands().as_slice(),
            [
                Command::Pan([100.0, -200.0, 300.0]),
                Command::FocusAll,
                Command::Pan([4.0, 5.0, 6.0])
            ]
        ));
    }

    #[test]
    fn resource_handles_resolve_wrappers_and_keep_raw_layers_and_errors() {
        let node = |name: &str, kind, buffer, children| crate::inspect::Node {
            name: name.into(),
            kind,
            buffer,
            range: 0..4,
            children,
            deferred: false,
            fields: Vec::new(),
            error: None,
        };
        let document = Arc::new(Document {
            root: 0,
            buffers: vec![
                Arc::from(*b"root"),
                Arc::from(*b"ecd!"),
                Arc::from(*b"exf!"),
                Arc::from(*b"jkr!"),
                Arc::from(*b"mesh"),
            ],
            nodes: vec![
                node("package", Kind::Archive, 0, vec![1, 5, 6, 7, 8]),
                node("encrypted", Kind::Ecd, 1, vec![2]),
                node("scrambled", Kind::Exf, 2, vec![3]),
                node("compressed", Kind::Jkr, 3, vec![4]),
                node("model", Kind::Fmod, 4, vec![]),
                node("reference", Kind::StageResourceReference, 0, vec![1]),
                node("original alias", Kind::Fmod, 4, vec![]),
                node("missing", Kind::Jkr, 3, vec![]),
                node("cycle A", Kind::Jkr, 3, vec![9]),
                node("cycle B", Kind::Exf, 2, vec![8]),
            ],
        });
        let source = |node| ResourceRef {
            document: document.clone(),
            node,
        };
        for index in [1, 2, 3, 4, 5] {
            assert_eq!(source(index).kind(), Kind::Fmod);
            assert_eq!(source(index).bytes().unwrap(), b"mesh");
            assert!(source(index).same_source(&source(4)));
        }
        assert_eq!(document.bytes(1), Some(b"ecd!".as_slice()));
        assert_eq!(document.bytes(3), Some(b"jkr!".as_slice()));
        assert!(!source(6).same_source(&source(4)));
        for index in [7, 8] {
            assert!(source(index).bytes().is_err());
            assert_eq!(source(index).kind(), Kind::Unknown);
        }
        for index in [1, 2, 3, 4, 5] {
            let mut invalid = (*document).clone();
            invalid.nodes[index].error = Some("original layer error".into());
            let invalid = ResourceRef {
                document: Arc::new(invalid),
                node: 5,
            };
            assert_eq!(invalid.bytes().unwrap_err(), "original layer error");
            assert_eq!(invalid.kind(), Kind::Unknown);
            assert!(!invalid.same_source(&source(4)));
        }
    }

    #[test]
    fn named_members_and_inner_geometry_expose_only_their_own_bundles() {
        let node = |name: &str, kind, children| crate::inspect::Node {
            name: name.into(),
            kind,
            buffer: 0,
            range: 0..16,
            children,
            deferred: false,
            fields: Vec::new(),
            error: None,
        };
        let document = Arc::new(Document {
            root: 0,
            buffers: vec![Arc::from([0_u8; 16])],
            nodes: vec![
                node("weapons.abn", Kind::Mha, vec![1, 8]),
                node("0000 · first.bin", Kind::Ecd, vec![2]),
                node("解码内容", Kind::Archive, vec![3, 6]),
                node("geometry", Kind::Archive, vec![4, 5]),
                node("model", Kind::Fmod, vec![]),
                node("skeleton", Kind::Fskl, vec![]),
                node("textures", Kind::Txb, vec![7]),
                node("image", Kind::Png, vec![]),
                node("0001 · second.bin", Kind::Ecd, vec![9]),
                node("解码内容", Kind::Archive, vec![10, 11, 12, 14]),
                node("model", Kind::Fmod, vec![]),
                node("skeleton", Kind::Fskl, vec![]),
                node("textures", Kind::Txb, vec![13]),
                node("image", Kind::Png, vec![]),
                node("motion", Kind::Motion, vec![]),
            ],
        });
        let (bundles, by_node) = AssetBundle::find_with_nodes(document);
        assert_eq!(bundles.len(), 2);
        assert_eq!(by_node[0], [0, 1]);
        for index in [1, 2, 3] {
            assert_eq!(by_node[index], [0]);
        }
        for index in [8, 9] {
            assert_eq!(by_node[index], [1]);
        }
        for index in [4, 5, 6, 7, 10, 11, 12, 13, 14] {
            assert!(by_node[index].is_empty());
        }
        assert!(bundles[0].name.ends_with("first.bin · 模型 1"));
        assert!(bundles[1].name.ends_with("second.bin · 模型 1"));
    }

    #[test]
    fn distinct_archive_entries_stay_distinct_when_their_bytes_are_aliased() {
        let node = || crate::inspect::Node {
            name: "aliased".into(),
            kind: Kind::Fmod,
            buffer: 0,
            range: 0..8,
            children: Vec::new(),
            deferred: false,
            fields: Vec::new(),
            error: None,
        };
        let document = Arc::new(Document {
            buffers: vec![Arc::from([0u8; 8])],
            nodes: vec![node(), node()],
            root: 0,
        });
        let first = ResourceRef {
            document: document.clone(),
            node: 0,
        };
        let second = ResourceRef {
            document: document.clone(),
            node: 1,
        };
        assert_eq!(first.bytes().unwrap(), second.bytes().unwrap());
        assert!(!first.same_source(&second));
        // Detail expansion clones metadata while retaining these buffer owners
        // and node indices; it must still select an existing loaded instance.
        let expanded = ResourceRef {
            document: Arc::new((*document).clone()),
            node: 0,
        };
        assert!(first.same_source(&expanded));
    }

    #[test]
    fn slider_coalescing_preserves_track_targets_and_animation_switch_order() {
        let control = Control::default();
        for frame in 0..100 {
            control
                .send(Command::Seek {
                    track: PlaybackTrack::Motion,
                    frame: frame as f32,
                })
                .unwrap();
        }
        let effect = PlaybackTrack::Effect {
            binding: 3,
            slot: 1,
        };
        control
            .send(Command::Seek {
                track: effect,
                frame: 4.0,
            })
            .unwrap();
        control
            .send(Command::Seek {
                track: effect,
                frame: 5.0,
            })
            .unwrap();
        control.send(Command::UnloadMotion).unwrap();
        control
            .send(Command::Seek {
                track: PlaybackTrack::Motion,
                frame: 7.0,
            })
            .unwrap();
        assert!(matches!(
            control.commands().as_slice(),
            [
                Command::Seek {
                    track: PlaybackTrack::Motion,
                    frame: 99.0
                },
                Command::Seek {
                    track: PlaybackTrack::Effect {
                        binding: 3,
                        slot: 1
                    },
                    frame: 5.0
                },
                Command::UnloadMotion,
                Command::Seek {
                    track: PlaybackTrack::Motion,
                    frame: 7.0
                }
            ]
        ));
    }

    #[test]
    fn closing_preempts_a_full_resource_queue_and_rejects_later_loads() {
        let control = Control::default();
        for id in 0..32 {
            control.send(Command::SelectModel(id)).unwrap();
        }
        assert!(control.send(Command::UnloadMotion).is_err());
        control.send(Command::Exit).unwrap();
        assert!(control.send(Command::LoadAssets(Vec::new())).is_err());
        control.send(Command::Exit).unwrap();
        assert!(control.closing());
        assert!(matches!(control.commands().as_slice(), [Command::Exit]));
        assert!(control.closing());
        assert!(control.send(Command::LoadAssets(Vec::new())).is_err());
        control.send(Command::Exit).unwrap();
        assert!(control.commands().is_empty());
    }

    #[test]
    fn mesh_commands_preserve_explicit_model_targets_and_order() {
        let control = Control::default();
        control
            .send(Command::IsolateMesh { model: 2, mesh: 3 })
            .unwrap();
        control.send(Command::SelectModel(1)).unwrap();
        control
            .send(Command::MeshVisible {
                model: 2,
                mesh: 1,
                visible: true,
            })
            .unwrap();
        control.send(Command::ShowAllMeshes(2)).unwrap();
        assert!(matches!(
            control.commands().as_slice(),
            [
                Command::IsolateMesh { model: 2, mesh: 3 },
                Command::SelectModel(1),
                Command::MeshVisible {
                    model: 2,
                    mesh: 1,
                    visible: true
                },
                Command::ShowAllMeshes(2),
            ]
        ));
    }
}
