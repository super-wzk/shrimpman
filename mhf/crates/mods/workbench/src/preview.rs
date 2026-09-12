//! Commands and owned snapshots cross the render/game thread boundary.

use crate::inspect::{Document, Kind};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

pub(crate) mod effects;
mod equipment;
mod resource_counts;
pub(crate) use resource_counts::loadable_resource_counts;
#[cfg(test)]
mod scope_tests;

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
    /// The actual loading path, including reference edges. Physical byte
    /// ownership is still resolved independently by `resource_node`.
    context: Vec<usize>,
}

impl ResourceRef {
    pub fn new(document: Arc<Document>, node: usize) -> Self {
        let context = document.metadata().path(node).unwrap_or_else(|| vec![node]);
        Self::at_context(document, node, context)
    }

    fn at_context(document: Arc<Document>, node: usize, mut context: Vec<usize>) -> Self {
        if let Some(target) = document.payload(node) {
            let mut current = node;
            while current != target {
                current = document.nodes[current].children[0];
                context.push(current);
            }
        }
        Self {
            document,
            node,
            context,
        }
    }

    pub fn scope(&self) -> crate::metadata::Scope<'_, '_> {
        crate::metadata::Scope::new(&self.document, &self.context)
    }

    pub fn scope_source(&self, node: usize) -> Self {
        let end = self
            .context
            .iter()
            .position(|&value| value == node)
            .expect("resolved metadata belongs to the loading path");
        Self::at_context(self.document.clone(), node, self.context[..=end].to_vec())
    }

    fn context_key(&self) -> Option<Vec<usize>> {
        if self.context.first() != Some(&self.document.root) {
            return None;
        }
        let mut key = Vec::new();
        for edge in self.context.windows(2) {
            let parent = self.document.nodes.get(edge[0])?;
            let ordinal = parent.children.iter().position(|&node| node == edge[1])?;
            if !parent.kind.is_transparent() {
                key.push(ordinal);
            }
        }
        Some(key)
    }

    fn children(&self) -> Vec<Self> {
        let Ok(payload) = resource_node(&self.document, self.node) else {
            return Vec::new();
        };
        self.document.nodes[payload]
            .children
            .iter()
            .map(|&node| {
                let mut context = self.context.clone();
                context.push(node);
                Self::at_context(self.document.clone(), node, context)
            })
            .collect()
    }

    /// Resolve a declared association in the current loading branch. A
    /// reference to a package keeps that package's children in its caller's
    /// scope rather than falling back to their unrelated physical ancestors.
    pub fn related(&self, node: usize) -> Option<Self> {
        // Tree traversal already knows the direct edge. Preserve its context
        // without rebuilding the document's physical parent index per row.
        for (prefix, &ancestor) in self.context.iter().enumerate().rev() {
            let context = if ancestor == node {
                self.context[..=prefix].to_vec()
            } else if self.document.nodes[ancestor].children.contains(&node) {
                let mut context = self.context[..=prefix].to_vec();
                context.push(node);
                context
            } else {
                continue;
            };
            return Some(Self::at_context(self.document.clone(), node, context));
        }
        self.related_in(node, &self.document.metadata())
    }

    fn related_in(&self, node: usize, scopes: &crate::metadata::Scopes<'_>) -> Option<Self> {
        let target = scopes.path(node)?;
        let current = &self.context;
        let (at, prefix) = target.iter().enumerate().rev().find_map(|(at, target)| {
            current
                .iter()
                .rposition(|value| value == target)
                .map(|prefix| (at, prefix))
        })?;
        let mut context = current[..=prefix].to_vec();
        context.extend_from_slice(&target[at + 1..]);
        Some(Self::at_context(self.document.clone(), node, context))
    }

    pub fn belongs_to(&self, path: &Path) -> bool {
        Path::new(&self.document.nodes[self.document.root].name) == path
    }

    /// Container member ordinals identify a loaded resource. Transparent
    /// payload links are rebuilt, so adding or removing encoding layers does
    /// not redirect an old payload path into the new resource's detail nodes.
    pub fn remap_path(&self, document: Arc<Document>) -> Result<Self, String> {
        let key = self.context_key().ok_or("已加载资源的原始路径失效")?;
        let selected_payload = self.document.payload(self.node) == Some(self.node);
        let mut replacement = Self::new(document.clone(), document.root);
        for ordinal in key {
            let payload = document
                .payload(replacement.node)
                .ok_or("编辑后资源路径中的包装或引用链失效")?;
            let node = document.nodes[payload]
                .children
                .get(ordinal)
                .copied()
                .ok_or_else(|| format!("编辑后找不到已加载资源：{}", self.short_name()))?;
            let mut context = replacement.context;
            context.push(node);
            replacement = Self::at_context(document.clone(), node, context);
        }
        if selected_payload {
            replacement.node = document
                .payload(replacement.node)
                .unwrap_or(replacement.node);
        }
        Ok(replacement)
    }

    pub fn remap(&self, document: Arc<Document>) -> Result<Self, String> {
        let replacement = self.remap_path(document)?;
        if replacement.kind() != self.kind() {
            return Err(format!("编辑后资源类型发生变化：{}", self.short_name()));
        }
        replacement.bytes()?;
        Ok(replacement)
    }

    pub fn same_origin(&self, other: &Self) -> bool {
        self.document.nodes[self.document.root].name
            == other.document.nodes[other.document.root].name
            && self.kind() == other.kind()
            && self
                .context_key()
                .is_some_and(|key| other.context_key().as_ref() == Some(&key))
    }

    /// Enumerate the selected subtree, stopping at complete resource boundaries.
    /// Model dependency associations never expand the selection's scope.
    pub fn loadable_resources(&self) -> Vec<Self> {
        let mut resources = Vec::new();
        let mut seen = HashSet::new();
        let mut pending = vec![self.clone()];
        while let Some(source) = pending.pop() {
            let Ok(node) = resource_node(&self.document, source.node) else {
                continue;
            };
            if !seen.insert((node, source.scope().origins())) {
                continue;
            }
            let value = &self.document.nodes[node];
            if is_loadable_resource(value.kind) {
                resources.push(source);
            } else {
                pending.extend(source.children().into_iter().rev());
            }
        }
        resources
    }

    pub fn texture_images(&self) -> Vec<Self> {
        if !matches!(self.kind(), Kind::Txb | Kind::Archive) {
            return vec![self.clone()];
        }
        self.children()
            .into_iter()
            .map(|source| {
                if self.document.nodes[source.node].range.is_empty() {
                    Self::white_texture()
                } else {
                    source
                }
            })
            .collect()
    }

    pub fn white_texture() -> Self {
        static WHITE: OnceLock<Arc<Document>> = OnceLock::new();
        let document = WHITE
            .get_or_init(|| {
                let mut bytes = vec![0u8; 132];
                bytes[..4].copy_from_slice(b"DDS ");
                for (at, value) in [
                    (4, 124u32),
                    (8, 0x100f),
                    (12, 1),
                    (16, 1),
                    (20, 4),
                    (76, 32),
                    (80, 0x41),
                    (88, 32),
                    (92, 0xff),
                    (96, 0xff00),
                    (100, 0xff0000),
                    (104, 0xff000000),
                    (108, 0x1000),
                    (128, u32::MAX),
                ] {
                    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
                }
                Arc::new(crate::inspect::inspect("默认白色贴图", bytes.into()))
            })
            .clone();
        Self::new(document.clone(), document.root)
    }

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

    /// Expanding a document clones its nodes but retains the original root bytes.
    pub fn same_document(&self, other: &Self) -> bool {
        let a = &self.document.nodes[self.document.root];
        let b = &other.document.nodes[other.document.root];
        a.kind == b.kind
            && a.range == b.range
            && Arc::ptr_eq(
                &self.document.buffers[a.buffer],
                &other.document.buffers[b.buffer],
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

    pub fn same_instance(&self, other: &Self) -> bool {
        self.same_source(other) && self.scope().origins() == other.scope().origins()
    }

    /// Independently loaded resources may associate when their shared defaults
    /// agree. A local declaration present on only one side is not a conflict.
    pub fn compatible_scope(&self, other: &Self) -> bool {
        if !self.same_document(other) {
            return false;
        }
        let first = self.scope().origins();
        let second = other.scope().origins();
        first.iter().all(|origin| {
            second
                .iter()
                .find(|other| other.type_id == origin.type_id)
                .is_none_or(|other| other.source == origin.source)
        })
    }

    pub fn kind(&self) -> Kind {
        resource_kind(&self.document, self.node)
    }
}

pub(crate) fn resource_kind(document: &Document, node: usize) -> Kind {
    resource_node(document, node).map_or(Kind::Unknown, |node| document.nodes[node].kind)
}

fn is_loadable_resource(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Fmod | Kind::Fskl | Kind::Png | Kind::Dds | Kind::Motion
    ) || effects::is_binding(kind)
        || effects::is_definition(kind)
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
    pub fn contains(&self, source: &ResourceRef) -> bool {
        self.model.same_instance(source)
            || self
                .skeleton
                .as_ref()
                .is_some_and(|skeleton| skeleton.same_instance(source))
            || self
                .textures
                .iter()
                .any(|texture| texture.same_instance(source))
    }

    pub fn loaded_from(&self, resources: &[LoadedResource]) -> Self {
        let loaded = |source: &ResourceRef| {
            resources
                .iter()
                .any(|entry| entry.enabled && entry.source.same_instance(source))
        };
        Self {
            model: self.model.clone(),
            name: self.name.clone(),
            skeleton: self.skeleton.clone().filter(&loaded),
            textures: self
                .textures
                .iter()
                .flat_map(ResourceRef::texture_images)
                .map(|source| {
                    if loaded(&source) {
                        source
                    } else {
                        ResourceRef::white_texture()
                    }
                })
                .collect(),
        }
    }
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

    pub fn same_instance(&self, other: &Self) -> bool {
        self.same_source(other)
            && self.model.same_instance(&other.model)
            && self
                .skeleton
                .iter()
                .zip(&other.skeleton)
                .all(|(a, b)| a.same_instance(b))
            && self
                .textures
                .iter()
                .zip(&other.textures)
                .all(|(a, b)| a.same_instance(b))
    }

    fn from_metadata(source: ResourceRef, scopes: &crate::metadata::Scopes<'_>) -> Self {
        let dependencies = source.scope().get::<crate::metadata::ModelResources>();
        let skeleton = dependencies
            .as_ref()
            .and_then(|value| value.value.skeleton)
            .and_then(|node| source.related_in(node, scopes));
        let textures = dependencies
            .into_iter()
            .flat_map(|value| &value.value.textures)
            .filter_map(|&node| source.related_in(node, scopes))
            .collect();
        Self {
            name: source.name(),
            model: source,
            skeleton,
            textures,
        }
    }

    pub fn from_source(source: ResourceRef, named: &[Self]) -> Self {
        let document = source.document.clone();
        let mut bundle = Self::from_metadata(source, &document.metadata());
        if let Some(named) = named
            .iter()
            .find(|candidate| candidate.same_source(&bundle))
        {
            bundle.name.clone_from(&named.name);
        }
        bundle
    }

    /// Format parsers declare associations; every resource consumes the same
    /// scope resolver. This index only provides browser grouping and names.
    pub fn find_with_nodes(document: Arc<Document>) -> (Vec<Self>, Vec<Vec<usize>>) {
        let scopes = document.metadata();
        let mut by_node = vec![Vec::new(); document.nodes.len()];
        let mut ordinals = HashMap::<usize, usize>::new();
        let mut result: Vec<Self> = Vec::new();
        for node in 0..document.nodes.len() {
            if resource_node(&document, node)
                .ok()
                .is_none_or(|payload| document.nodes[payload].kind != Kind::Fmod)
            {
                continue;
            }
            let Some(context) = scopes.path(node) else {
                continue;
            };
            let mut source = ResourceRef::at_context(document.clone(), node, context);
            source.node = *source.context.last().unwrap();
            if source
                .scope()
                .get::<crate::metadata::ModelResources>()
                .is_none()
            {
                continue;
            }
            let mut bundle = Self::from_metadata(source, &scopes);
            let index = if let Some(index) = result.iter().position(|old| old.same_source(&bundle))
            {
                index
            } else {
                let named = bundle
                    .model
                    .context
                    .windows(2)
                    .rev()
                    .find(|edge| document.nodes[edge[0]].kind == Kind::Mha)
                    .map_or(document.root, |edge| edge[1]);
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
                result.push(bundle.clone());
                index
            };
            for &ancestor in &bundle.model.context {
                if resource_node(&document, ancestor)
                    .is_ok_and(|node| document.nodes[node].kind == Kind::Fmod)
                {
                    continue;
                }
                if !by_node[ancestor].contains(&index) {
                    by_node[ancestor].push(index);
                }
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
pub(crate) struct LoadedResource {
    pub id: u64,
    pub source: ResourceRef,
    pub enabled: bool,
}

impl LoadedResource {
    pub fn in_category(&self, kind: Kind) -> bool {
        if kind == Kind::Txb {
            matches!(self.source.kind(), Kind::Png | Kind::Dds)
        } else {
            self.source.kind() == kind
        }
    }
}

#[derive(Clone)]
pub(crate) struct LoadedModel {
    pub id: u64,
    pub resources: AssetBundle,
    pub name: Arc<str>,
    pub visible: bool,
    pub error: Option<Arc<str>>,
    pub meshes: Arc<Vec<LoadedMesh>>,
}

#[derive(Clone)]
pub(crate) struct LoadedSkeleton {
    pub id: u64,
    pub bones: Arc<Vec<Bone>>,
    pub bone_bindings: Arc<Vec<Option<usize>>>,
    pub error: Option<Arc<str>>,
}

#[derive(Clone)]
pub(crate) struct LoadedMotion {
    pub id: u64,
    pub source: ResourceRef,
    pub enabled: bool,
    pub frames: f32,
    pub frame: Option<f32>,
    pub skeleton: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct LoadedEffect {
    pub id: u64,
    pub source: ResourceRef,
    pub enabled: bool,
    pub model: Option<u64>,
    pub automatic: bool,
    pub manual_target: Option<u64>,
    pub model_id: Option<u16>,
    pub message: Arc<str>,
    pub binding: effects::BindingSnapshot,
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub ready: bool,
    pub playing: bool,
    pub playback_speed: f32,
    pub camera: Option<Camera>,
    /// Actual pixel-rounded rectangle from the same rendered frame as camera.
    pub viewport: Viewport,
    pub distance: f32,
    pub pitch: f32,
    pub yaw: f32,
    pub message: Arc<str>,
    pub models: Arc<Vec<LoadedModel>>,
    pub resources: Arc<Vec<LoadedResource>>,
    pub motions: Arc<Vec<LoadedMotion>>,
    pub skeletons: Arc<Vec<LoadedSkeleton>>,
    pub loaded_effects: Arc<Vec<LoadedEffect>>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            ready: false,
            playing: true,
            playback_speed: 1.0,
            camera: None,
            viewport: Viewport::default(),
            distance: 350.0,
            pitch: 10.0,
            yaw: 0.0,
            message: "正在初始化资源工作台".into(),
            models: Arc::new(Vec::new()),
            resources: Arc::default(),
            motions: Arc::default(),
            skeletons: Arc::default(),
            loaded_effects: Arc::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaybackTrack {
    Motion(u64),
    Effect { binding: u64, slot: usize },
}

pub(crate) enum Command {
    RefreshDocument {
        path: PathBuf,
        document: Arc<Document>,
    },
    LoadResource(ResourceRef),
    RemoveResource(u64),
    ResourceEnabled {
        id: u64,
        enabled: bool,
    },
    ClearResources(Kind),
    BindEffect {
        binding: u64,
        model: Option<u64>,
    },
    AutoBindEffect(u64),
    EffectEnabled {
        binding: u64,
        enabled: bool,
    },
    RemoveEffect(u64),
    ClearLoadedEffects,
    TriggerEffectDefinition {
        binding: u64,
        slot: usize,
    },
    StopEffectDefinition {
        binding: u64,
        slot: usize,
    },
    RemoveEffectDefinition {
        binding: u64,
        slot: usize,
    },
    RemoveAsset(u64),
    ClearAssets,
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
        skeleton: u64,
        node: usize,
        source: Option<usize>,
    },
    ClearBoneBindings(u64),
    FocusAll,
    RemoveMotion(u64),
    ClearMotions,
    MotionEnabled {
        id: u64,
        enabled: bool,
    },
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
    FocusBone {
        skeleton: u64,
        node: Option<usize>,
    },
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
                (
                    Command::RefreshDocument { path: first, .. },
                    Command::RefreshDocument { path: second, .. },
                ) => first == second,
                (Command::Seek { track: first, .. }, Command::Seek { track: second, .. }) => {
                    first == second
                }
                (Command::PlaybackSpeed(_), Command::PlaybackSpeed(_))
                | (Command::Camera { .. }, Command::Camera { .. }) => true,
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

    fn metadata_fixture(mut document: Document) -> Arc<Document> {
        for (scope, value) in crate::metadata::model_resources(&document) {
            document.nodes[scope].metadata.insert(value);
        }
        Arc::new(document)
    }

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
            metadata: Default::default(),
            error: None,
        };
        let document = metadata_fixture(Document {
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
        let source = |node| ResourceRef::new(document.clone(), node);
        assert_eq!(
            source(0)
                .loadable_resources()
                .iter()
                .map(|source| source.node)
                .collect::<Vec<_>>(),
            [1, 6]
        );
        let counts = loadable_resource_counts(&document);
        assert_eq!(counts, [2, 1, 1, 1, 1, 1, 1, 0, 0, 0]);
        for (index, count) in counts.into_iter().enumerate() {
            assert_eq!(count, source(index).loadable_resources().len());
        }
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
            let invalid = ResourceRef::new(Arc::new(invalid), 5);
            assert_eq!(invalid.bytes().unwrap_err(), "original layer error");
            assert_eq!(invalid.kind(), Kind::Unknown);
            assert!(!invalid.same_source(&source(4)));
            assert!(invalid.loadable_resources().is_empty());
            assert_eq!(loadable_resource_counts(&invalid.document)[5], 0);
        }
    }

    #[test]
    fn directory_counts_keep_reference_owners_and_stop_at_resource_boundaries() {
        let node = |kind, children| crate::inspect::Node {
            name: String::new(),
            kind,
            buffer: 0,
            range: 0..4,
            children,
            deferred: false,
            fields: Vec::new(),
            metadata: Default::default(),
            error: None,
        };
        let document = metadata_fixture(Document {
            root: 0,
            buffers: vec![Arc::from(*b"data")],
            nodes: vec![
                node(Kind::Archive, vec![1, 6, 11]),
                node(Kind::StageObjectPackage, vec![2, 3, 4]),
                node(Kind::Fmod, vec![12]),
                node(Kind::Fskl, vec![]),
                node(Kind::Txb, vec![5]),
                node(Kind::Dds, vec![]),
                node(Kind::StageObjectPackage, vec![7, 8, 9]),
                node(Kind::StageResourceReference, vec![2]),
                node(Kind::Fskl, vec![]),
                node(Kind::Txb, vec![10]),
                node(Kind::Png, vec![]),
                node(Kind::Motion, vec![13]),
                node(Kind::Png, vec![]),
                node(Kind::Motion, vec![]),
            ],
        });
        let sources = |node| ResourceRef::new(document.clone(), node).loadable_resources();
        assert_eq!(
            sources(6)
                .iter()
                .map(|source| source.node)
                .collect::<Vec<_>>(),
            [7, 8, 10]
        );
        assert_eq!(
            sources(0)
                .iter()
                .map(|source| source.node)
                .collect::<Vec<_>>(),
            [2, 3, 5, 8, 10, 11]
        );
        for (index, count) in loadable_resource_counts(&document).into_iter().enumerate() {
            assert_eq!(count, sources(index).len(), "node {index}");
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
            metadata: Default::default(),
            error: None,
        };
        let document = metadata_fixture(Document {
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
        let other_skeleton = bundles[1].skeleton.clone().unwrap();
        let image = bundles[0].textures[0].texture_images().remove(0);
        let loaded = vec![
            LoadedResource {
                id: 1,
                source: other_skeleton,
                enabled: true,
            },
            LoadedResource {
                id: 2,
                source: image.clone(),
                enabled: true,
            },
        ];
        let first = bundles[0].loaded_from(&loaded);
        assert!(
            first.skeleton.is_none(),
            "unrelated skeletons must not bind by node numbers"
        );
        assert!(first.textures[0].same_source(&image));
        assert!(bundles[1].loaded_from(&loaded).skeleton.is_some());
    }

    #[test]
    fn txb_images_load_individually_and_missing_images_keep_their_original_slots() {
        let node = |kind, children| crate::inspect::Node {
            name: "resource".into(),
            kind,
            buffer: 0,
            range: 0..16,
            children,
            deferred: false,
            fields: Vec::new(),
            metadata: Default::default(),
            error: None,
        };
        let document = metadata_fixture(Document {
            root: 0,
            buffers: vec![Arc::from([0u8; 16])],
            nodes: vec![
                node(Kind::Archive, vec![1, 2]),
                node(Kind::Fmod, vec![]),
                node(Kind::Txb, vec![3, 4, 5]),
                node(Kind::Png, vec![]),
                node(Kind::Png, vec![]),
                node(Kind::Dds, vec![]),
            ],
        });
        let source = |node| ResourceRef::new(document.clone(), node);
        let images = source(2).texture_images();
        assert_eq!(
            images.iter().map(|image| image.node).collect::<Vec<_>>(),
            [3, 4, 5]
        );
        let group = AssetBundle {
            name: "group".into(),
            model: source(1),
            skeleton: None,
            textures: vec![source(2)],
        };
        let mut loaded: Vec<_> = images
            .iter()
            .enumerate()
            .map(|(index, source)| LoadedResource {
                id: index as u64,
                source: source.clone(),
                enabled: true,
            })
            .collect();
        assert!(
            group
                .loaded_from(&loaded)
                .textures
                .iter()
                .zip(&images)
                .all(|(a, b)| a.same_source(b))
        );
        loaded.remove(1);
        let partial = group.loaded_from(&loaded);
        assert_eq!(partial.textures.len(), 3);
        assert!(partial.textures[0].same_source(&images[0]));
        assert!(partial.textures[1].same_source(&ResourceRef::white_texture()));
        assert!(partial.textures[2].same_source(&images[2]));
        loaded[1].enabled = false;
        let disabled = group.loaded_from(&loaded);
        assert!(disabled.textures[0].same_source(&images[0]));
        assert!(disabled.textures[2].same_source(&ResourceRef::white_texture()));
        loaded[1].enabled = true;
        assert!(
            partial.same_source(&group.loaded_from(&loaded)),
            "defaults remain stable across reconciliation"
        );
        assert_eq!(source(2).texture_images().len(), 3);
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
            metadata: Default::default(),
            error: None,
        };
        let document = metadata_fixture(Document {
            buffers: vec![Arc::from([0u8; 8])],
            nodes: vec![node(), node()],
            root: 0,
        });
        let first = ResourceRef::new(document.clone(), 0);
        let second = ResourceRef::new(document.clone(), 1);
        assert_eq!(first.bytes().unwrap(), second.bytes().unwrap());
        assert!(!first.same_source(&second));
        // Detail expansion clones metadata while retaining these buffer owners
        // and node indices; it must still select an existing loaded instance.
        let expanded = ResourceRef::new(Arc::new((*document).clone()), 0);
        assert!(first.same_source(&expanded));
    }

    #[test]
    fn consecutive_document_edits_coalesce_without_crossing_load_operations() {
        let control = Control::default();
        let first = Arc::new(crate::inspect::inspect("edited.bin", vec![1u8].into()));
        let latest = Arc::new(crate::inspect::inspect("edited.bin", vec![2u8].into()));
        for document in [first, latest.clone()] {
            control
                .send(Command::RefreshDocument {
                    path: "edited.bin".into(),
                    document,
                })
                .unwrap();
        }
        control
            .send(Command::LoadResource(ResourceRef::new(
                latest.clone(),
                latest.root,
            )))
            .unwrap();
        control
            .send(Command::RefreshDocument {
                path: "edited.bin".into(),
                document: latest.clone(),
            })
            .unwrap();
        let commands = control.commands();
        assert_eq!(commands.len(), 3);
        let Command::RefreshDocument { document, .. } = &commands[0] else {
            panic!()
        };
        assert!(Arc::ptr_eq(document, &latest));
        assert!(matches!(commands[1], Command::LoadResource(_)));
    }

    #[test]
    fn slider_coalescing_preserves_track_targets_and_animation_switch_order() {
        let control = Control::default();
        for frame in 0..100 {
            control
                .send(Command::Seek {
                    track: PlaybackTrack::Motion(5),
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
        control.send(Command::RemoveMotion(5)).unwrap();
        control
            .send(Command::Seek {
                track: PlaybackTrack::Motion(5),
                frame: 7.0,
            })
            .unwrap();
        assert!(matches!(
            control.commands().as_slice(),
            [
                Command::Seek {
                    track: PlaybackTrack::Motion(5),
                    frame: 99.0
                },
                Command::Seek {
                    track: PlaybackTrack::Effect {
                        binding: 3,
                        slot: 1
                    },
                    frame: 5.0
                },
                Command::RemoveMotion(5),
                Command::Seek {
                    track: PlaybackTrack::Motion(5),
                    frame: 7.0
                }
            ]
        ));
    }

    #[test]
    fn closing_preempts_a_full_resource_queue_and_rejects_later_loads() {
        let control = Control::default();
        for id in 0..32 {
            control.send(Command::RemoveAsset(id)).unwrap();
        }
        assert!(control.send(Command::RemoveMotion(5)).is_err());
        control.send(Command::Exit).unwrap();
        assert!(
            control
                .send(Command::LoadResource(ResourceRef::white_texture()))
                .is_err()
        );
        control.send(Command::Exit).unwrap();
        assert!(control.closing());
        assert!(matches!(control.commands().as_slice(), [Command::Exit]));
        assert!(control.closing());
        assert!(
            control
                .send(Command::LoadResource(ResourceRef::white_texture()))
                .is_err()
        );
        control.send(Command::Exit).unwrap();
        assert!(control.commands().is_empty());
    }

    #[test]
    fn mesh_commands_preserve_explicit_model_targets_and_order() {
        let control = Control::default();
        control
            .send(Command::IsolateMesh { model: 2, mesh: 3 })
            .unwrap();
        control.send(Command::RemoveAsset(1)).unwrap();
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
                Command::RemoveAsset(1),
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
