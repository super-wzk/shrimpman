//! Read-only DAT bindings and deterministic, instance-local inspection playback.
//! Game equipment predicates and the attachment resource registry are not invoked.

use super::{Kind, ResourceRef};
use mhf_resource::{
    dat::{self, Dat},
    effect::{
        AttachmentDefinition, AttachmentGroup, ModelEffectBinding, ModelEffectDefinition,
        UvAnimation,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT_BINDING_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TRIGGER_ORDER: AtomicU64 = AtomicU64::new(1);

pub(crate) fn is_binding(kind: Kind) -> bool {
    matches!(kind, Kind::DatRecord(index) if index == dat::DATA_TABLES.len() || index == dat::DATA_TABLES.len() + 2)
}

pub(crate) fn is_definition(kind: Kind) -> bool {
    matches!(kind, Kind::DatRecord(index) if index == dat::DATA_TABLES.len() + 1 || index == dat::DATA_TABLES.len() + 3)
}

#[derive(Clone)]
pub(crate) enum Definition {
    Attachment(AttachmentDefinition),
    Model(ModelEffectDefinition),
}

impl Definition {
    fn timeline(&self) -> (f32, bool) {
        let mut duration = 1;
        let mut looping = false;
        if let Self::Model(value) = self {
            let mut include = |steps: u32, repetitions: i16| {
                if steps != 0 && repetitions != 0 {
                    duration = duration.max(steps * repetitions.max(1) as u32);
                    looping |= repetitions < 0;
                }
            };
            for channel in &value.rotation {
                include(channel_period(channel.duration, false), channel.repetitions);
            }
            for channel in &value.scale {
                include(
                    channel_period(channel.duration, channel.ping_pong != 0),
                    channel.repetitions,
                );
            }
            include(
                channel_period(value.color.duration, value.color.ping_pong != 0),
                value.color.repetitions,
            );
            include(
                channel_period(value.opacity.duration, value.opacity.ping_pong != 0),
                value.opacity.repetitions,
            );
            if value.uv.mode == 0 {
                include(value.uv.cycle_steps.into(), value.uv.repetitions);
            }
        }
        (f32::from(self.delay()) + duration as f32, looping)
    }

    fn transforms_node(&self) -> bool {
        match self {
            Self::Attachment(_) => false,
            Self::Model(value) => {
                value.translation_delta().iter().any(|&value| value != 0.0)
                    || value.rotation.iter().any(|channel| {
                        channel.start_degrees != 0
                            || channel.repetitions != 0
                                && channel.duration != 0
                                && channel.end_degrees != 0
                    })
                    || value.scale.iter().any(|channel| {
                        f32::from_bits(channel.start_bits) != 1.0
                            || channel.repetitions != 0
                                && channel.duration != 0
                                && f32::from_bits(channel.end_bits) != 1.0
                    })
            }
        }
    }
    fn node(&self) -> usize {
        usize::from(match self {
            Self::Attachment(value) => value.node_index,
            Self::Model(value) => value.node_index,
        })
    }

    fn delay(&self) -> u16 {
        match self {
            Self::Attachment(value) => value.start_delay,
            Self::Model(value) => value.start_delay,
        }
    }

    fn position(&self) -> [f32; 3] {
        match self {
            Self::Attachment(value) => value.local_position(),
            Self::Model(value) => value.translation_delta(),
        }
    }

    pub(crate) fn draw(&self) -> Option<(usize, usize)> {
        match self {
            Self::Attachment(_) => None,
            Self::Model(value) => Some((value.draw_group.into(), value.group_entry.into())),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub slot: usize,
    pub id: u16,
    pub started_at: Option<f32>,
    pub trigger_order: u64,
    pub definition: Definition,
}

#[derive(Clone)]
pub(crate) struct Binding {
    pub id: u64,
    pub source: ResourceRef,
    pub entries: Vec<Entry>,
}

impl Binding {
    pub fn read(source: ResourceRef) -> Result<Self, String> {
        if !is_binding(source.kind()) && !is_definition(source.kind()) {
            return Err("请选择 DAT 特效定义或绑定记录".into());
        }
        let document = &source.document;
        let index = document.payload(source.node).ok_or("特效绑定节点失效")?;
        // Follow the actual tree, so an embedded DAT uses its own image base.
        let mut parents = vec![None; document.nodes.len()];
        for (parent, node) in document.nodes.iter().enumerate() {
            if node.kind != Kind::StageResourceReference {
                for &child in &node.children {
                    if let Some(value) = parents.get_mut(child) {
                        *value = Some(parent);
                    }
                }
            }
        }
        let mut owner = parents[index];
        let mut found = None;
        for _ in &document.nodes {
            let Some(parent) = owner else { break };
            if document.nodes[parent].kind == Kind::Dat {
                found = Some(parent);
                break;
            }
            owner = parents[parent];
        }
        let owner = found.ok_or("特效绑定缺少所属 DAT 文件")?;
        let root = &document.nodes[owner];
        let node = &document.nodes[index];
        let file = Dat::parse(document.bytes(owner).ok_or("DAT 数据失效")?)
            .map_err(|error| error.to_string())?;
        let Kind::DatRecord(table_index) = node.kind else {
            unreachable!()
        };
        let table_index = table_index - dat::DATA_TABLES.len();
        let table = file
            .table(&dat::EFFECT_TABLES[table_index])
            .map_err(|error| error.to_string())?;
        let offset = node
            .range
            .start
            .checked_sub(root.range.start)
            .ok_or("绑定记录偏移无效")?;
        if node.buffer != root.buffer
            || offset < table.range.start
            || !(offset - table.range.start).is_multiple_of(usize::from(table.layout.stride))
        {
            return Err("绑定记录不属于对应 DAT 表".into());
        }
        let (_, bytes) = table
            .record((offset - table.range.start) / usize::from(table.layout.stride))
            .map_err(|error| error.to_string())?;
        if node.range.len() != bytes.len() {
            return Err("绑定记录范围不完整".into());
        }
        let standalone = table_index % 2 == 1;
        let ids = if standalone {
            vec![
                u16::try_from((offset - table.range.start) / usize::from(table.layout.stride))
                    .map_err(|_| "特效定义 ID 超出范围")?,
            ]
        } else if table_index == 0 {
            AttachmentGroup::parse(bytes)
                .map_err(|error| error.to_string())?
                .active_definition_ids()
                .to_vec()
        } else {
            ModelEffectBinding::parse(bytes)
                .map_err(|error| error.to_string())?
                .active_definition_ids()
                .to_vec()
        };
        if ids.is_empty() {
            return Err("此绑定没有有效特效定义（定义列表在首个零处结束）".into());
        }
        let definitions = file
            .table(&dat::EFFECT_TABLES[table_index | 1])
            .map_err(|error| error.to_string())?;
        let entries = ids
            .into_iter()
            .enumerate()
            .map(|(slot, id)| {
                let (_, bytes) = definitions
                    .record(id.into())
                    .map_err(|error| format!("绑定槽 {slot} · 定义 {id}：{error}"))?;
                let definition = if table_index < 2 {
                    Definition::Attachment(
                        AttachmentDefinition::parse(bytes).map_err(|error| error.to_string())?,
                    )
                } else {
                    Definition::Model(
                        ModelEffectDefinition::parse(bytes).map_err(|error| error.to_string())?,
                    )
                };
                Ok(Entry {
                    slot,
                    id,
                    started_at: None,
                    trigger_order: 0,
                    definition,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            id: NEXT_BINDING_ID.fetch_add(1, Ordering::Relaxed),
            source,
            entries,
        })
    }
}

#[derive(Default)]
pub(crate) struct Target {
    pub nodes: usize,
    /// Native mesh ordinal -> number of local material slots, not material IDs.
    pub material_counts: Vec<usize>,
}

impl Target {
    fn error(&self, definition: &Definition) -> Option<String> {
        let node = definition.node();
        if node >= self.nodes {
            return Some(format!("节点 {node} 超出当前骨架范围（{}）", self.nodes));
        }
        if let Some((mesh, entry)) = definition.draw()
            && self
                .material_counts
                .get(mesh)
                .is_none_or(|&count| entry >= count)
        {
            return Some(format!("绘制组 {mesh} / 条目 {entry} 不存在于当前模型"));
        }
        if definition.position().iter().any(|value| !value.is_finite()) {
            return Some("位移包含非有限值，保留原值但不应用".into());
        }
        if let Definition::Model(value) = definition
            && value.scale.iter().any(|channel| {
                !f32::from_bits(channel.start_bits).is_finite()
                    || channel.repetitions != 0
                        && channel.duration != 0
                        && !f32::from_bits(channel.end_bits).is_finite()
            })
        {
            return Some("缩放包含非有限值，保留原值但不应用".into());
        }
        None
    }
}

#[derive(Clone)]
pub(crate) struct DefinitionSnapshot {
    pub slot: usize,
    pub id: u16,
    pub condition: u8,
    pub node: usize,
    pub draw: Option<(usize, usize)>,
    pub delay: u16,
    pub frames: f32,
    pub frame: Option<f32>,
    pub looping: bool,
    pub active: bool,
    pub message: String,
    pub position: Option<[f32; 3]>,
    pub color: [u8; 3],
}

#[derive(Clone)]
pub(crate) struct BindingSnapshot {
    pub id: u64,
    pub name: String,
    pub definitions: Vec<DefinitionSnapshot>,
}

pub(crate) struct MaterialSample {
    pub mesh: usize,
    pub entry: usize,
    pub rgb: [f32; 3],
    pub opacity: f32,
    pub uv_offset: Option<[f32; 2]>,
    pub render_flags: u8,
    pub render_state: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NodeTransform {
    pub mesh: usize,
    pub node: usize,
    pub translation: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl NodeTransform {
    fn sample(value: &ModelEffectDefinition, step: u32) -> Result<Self, String> {
        let rotation = value.rotation.each_ref().map(|channel| {
            let t = channel_phase(step, channel.duration, channel.repetitions, false);
            let start = f32::from(channel.start_degrees);
            start + (f32::from(channel.end_degrees) - start) * t
        });
        let scale = value.scale.each_ref().map(|channel| {
            let start = f32::from_bits(channel.start_bits);
            if channel.repetitions == 0 || channel.duration == 0 {
                return start;
            }
            let end = f32::from_bits(channel.end_bits);
            let t = channel_phase(
                step,
                channel.duration,
                channel.repetitions,
                channel.ping_pong != 0,
            );
            (1.0 - t) * start + t * end
        });
        let translation = value.translation_delta();
        if rotation
            .iter()
            .chain(&scale)
            .chain(&translation)
            .any(|value| !value.is_finite())
        {
            return Err("特效变换包含非有限值，保留原值但不应用".into());
        }
        Ok(Self {
            mesh: value.draw_group.into(),
            node: value.node_index.into(),
            translation,
            rotation,
            scale,
        })
    }

    /// 10BBE150 rebuilds S * Rx * Ry * Rz at node +64, retaining its
    /// translation. Match 10004F80's row-vector order on the preview copy.
    pub fn apply(self, local: &mut [f32; 16]) {
        if self.scale != [1.0; 3] || self.rotation != [0.0; 3] {
            let mut matrix = [0.0; 16];
            for axis in 0..3 {
                matrix[axis * 5] = self.scale[axis];
            }
            let [x, y, z] = self.rotation.map(f32::to_radians);
            let (sx, cx) = x.sin_cos();
            let (sy, cy) = y.sin_cos();
            let (sz, cz) = z.sin_cos();
            for row in 0..3 {
                let at = row * 4;
                let [a, b, c] = [matrix[at], matrix[at + 1], matrix[at + 2]];
                let (b, c) = (b * cx - c * sx, c * cx + b * sx);
                let (a, c) = (c * sy + a * cy, c * cy - a * sy);
                local[at..at + 3].copy_from_slice(&[a * cz - b * sz, b * cz + a * sz, c]);
            }
        }
        for axis in 0..3 {
            local[12 + axis] += self.translation[axis];
        }
    }
}

#[derive(Default)]
pub(crate) struct Sample {
    pub bindings: Vec<BindingSnapshot>,
    pub transforms: Vec<NodeTransform>,
    pub materials: Vec<MaterialSample>,
    attachments: Vec<(usize, usize, usize, [f32; 3])>,
}

impl Sample {
    pub fn place_mesh(&mut self, mesh: usize, worlds: &[[f32; 16]]) {
        for entry in self
            .bindings
            .iter_mut()
            .flat_map(|binding| &mut binding.definitions)
        {
            if entry.active && entry.draw.is_some_and(|(group, _)| group == mesh) {
                entry.position = worlds
                    .get(entry.node)
                    .map(|world| [world[12], world[13], world[14]]);
            }
        }
    }
    pub fn place(&mut self, worlds: &[[f32; 16]]) {
        for binding in &mut self.bindings {
            for entry in &mut binding.definitions {
                if entry.active && entry.draw.is_some() {
                    entry.position = worlds
                        .get(entry.node)
                        .map(|world| [world[12], world[13], world[14]]);
                }
            }
        }
        for &(binding, entry, node, local) in &self.attachments {
            if let Some(world) = worlds.get(node) {
                let position = std::array::from_fn(|axis| {
                    world[12 + axis] + (0..3).map(|i| local[i] * world[i * 4 + axis]).sum::<f32>()
                });
                let snapshot = &mut self.bindings[binding].definitions[entry];
                if position.iter().all(|value| value.is_finite()) {
                    snapshot.position = Some(position);
                } else {
                    snapshot.active = false;
                    snapshot.message = "附着点变换产生非有限值".into();
                }
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Effects {
    pub bindings: Vec<Binding>,
    pub target: Target,
    pub snapshot: Arc<Vec<BindingSnapshot>>,
}

impl Effects {
    pub fn trigger(&mut self, binding: u64, slot: usize, frame: f32) -> Result<(), String> {
        if !frame.is_finite() || frame < 0.0 {
            return Err("触发位置无效".into());
        }
        let entry = self.entry_mut(binding, slot)?;
        entry.started_at = Some(frame);
        entry.trigger_order = NEXT_TRIGGER_ORDER.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop(&mut self, binding: u64, slot: usize) -> Result<(), String> {
        self.entry_mut(binding, slot)?.started_at = None;
        Ok(())
    }

    fn entry_mut(&mut self, binding: u64, slot: usize) -> Result<&mut Entry, String> {
        self.bindings
            .iter_mut()
            .find(|value| value.id == binding)
            .ok_or("特效来源已移除")?
            .entries
            .iter_mut()
            .find(|entry| entry.slot == slot)
            .ok_or_else(|| "特效定义已移除".into())
    }

    pub fn seek(
        &mut self,
        binding: u64,
        slot: usize,
        clock: f32,
        frame: f32,
    ) -> Result<(), String> {
        let entry = self.entry_mut(binding, slot)?;
        if !frame.is_finite() || frame < 0.0 || frame > entry.definition.timeline().0 {
            return Err("特效轨道步数超出范围".into());
        }
        let start = entry.started_at.as_mut().ok_or("请先触发此特效")?;
        *start = clock - frame;
        Ok(())
    }

    pub fn step(&mut self, binding: u64, slot: usize, clock: f32, delta: i8) -> Result<(), String> {
        let entry = self.entry_mut(binding, slot)?;
        let (frames, looping) = entry.definition.timeline();
        let start = entry.started_at.as_mut().ok_or("请先触发此特效")?;
        let mut local = (clock - *start).max(0.0);
        if !looping {
            local = local.min(frames);
        }
        *start = clock - (local + f32::from(delta)).max(0.0);
        Ok(())
    }

    pub fn sample(&self, frame: f32) -> Sample {
        let mut sample = Sample::default();
        for (binding_index, binding) in self.bindings.iter().enumerate() {
            let mut snapshots = Vec::new();
            for (entry_index, entry) in binding.entries.iter().enumerate() {
                let definition = &entry.definition;
                let node = definition.node();
                let draw = definition.draw();
                let position = definition.position();
                let (frames, looping) = definition.timeline();
                let mut snapshot = DefinitionSnapshot {
                    slot: entry.slot,
                    id: entry.id,
                    condition: match definition {
                        Definition::Attachment(value) => value.activation_condition,
                        Definition::Model(value) => value.activation_condition,
                    },
                    node,
                    draw,
                    delay: definition.delay(),
                    frames,
                    frame: entry.started_at.map(|start| {
                        let local = (frame - start).max(0.0);
                        let delay = f32::from(definition.delay());
                        if looping && local >= frames {
                            delay + (local - delay).rem_euclid(frames - delay)
                        } else {
                            local.min(frames)
                        }
                    }),
                    looping,
                    active: false,
                    message: String::new(),
                    position: None,
                    color: [255, 190, 55],
                };
                let overridden = self
                    .bindings
                    .iter()
                    .flat_map(|binding| &binding.entries)
                    .any(|other| {
                        let (Some((mesh, local)), Some((other_mesh, other_local))) =
                            (draw, other.definition.draw())
                        else {
                            return false;
                        };
                        let same_target = mesh == other_mesh
                            && (local == other_local
                                || definition.transforms_node()
                                    && other.definition.transforms_node());
                        other.trigger_order > entry.trigger_order
                            && other.started_at.is_some_and(|start| {
                                frame >= start + f32::from(other.definition.delay())
                            })
                            && self.target.error(&other.definition).is_none()
                            && same_target
                    });
                let start = entry.started_at.unwrap_or(0.0);
                let due = start + f32::from(definition.delay());
                snapshot.message = if entry.started_at.is_none() {
                    "尚未触发；点击触发从局部第 0 步开始".into()
                } else if let Some(error) = self.target.error(definition) {
                    error
                } else if !frame.is_finite() || frame < 0.0 {
                    "预览步数无效".into()
                } else if frame < start {
                    format!("尚未到触发位置 · 第 {start:.0} 步")
                } else if frame < due {
                    format!("等待启动 · 剩余 {:.0} 步", due - frame)
                } else if overridden {
                    "同一目标由最近触发的定义预览；可点击重播".into()
                } else {
                    match definition {
                        Definition::Attachment(value) if value.attachment_mode != 0 => {
                            format!(
                                "附着模式 {} 依赖游戏实例，尚不能定位",
                                value.attachment_mode
                            )
                        }
                        Definition::Attachment(value) => {
                            snapshot.active = true;
                            sample
                                .attachments
                                .push((binding_index, entry_index, node, position));
                            format!("附着点预览 · 资源 {} 尚未接入注册表", value.resource_id)
                        }
                        Definition::Model(value) => {
                            let step = (frame - due).floor() as u32;
                            let transform = match NodeTransform::sample(value, step) {
                                Ok(transform) => transform,
                                Err(error) => {
                                    snapshot.message = error;
                                    snapshots.push(snapshot);
                                    continue;
                                }
                            };
                            snapshot.active = true;
                            let color_phase = channel_phase(
                                step,
                                value.color.duration,
                                value.color.repetitions,
                                value.color.ping_pong != 0,
                            );
                            let rgb = std::array::from_fn(|axis| {
                                let start = f32::from(value.color.start_rgb[axis]);
                                let end = f32::from(value.color.end_rgb[axis]);
                                (start + (end - start) * color_phase) as u8
                            });
                            let opacity_phase = channel_phase(
                                step,
                                value.opacity.duration,
                                value.opacity.repetitions,
                                value.opacity.ping_pong != 0,
                            );
                            let start = f32::from(value.opacity.start);
                            let end = f32::from(value.opacity.end);
                            // Native rendering truncates to BYTE; the source remains u16.
                            let opacity = (start + (end - start) * opacity_phase) as u16 as u8;
                            let uv_offset = sample_uv(&value.uv, step);
                            snapshot.color = rgb;
                            if definition.transforms_node() {
                                sample.transforms.push(transform);
                            }
                            sample.materials.push(MaterialSample {
                                mesh: value.draw_group.into(),
                                entry: value.group_entry.into(),
                                rgb: rgb.map(|value| f32::from(value) / 255.0),
                                opacity: f32::from(opacity) / 255.0,
                                uv_offset,
                                render_flags: value.render_flags,
                                render_state: value.render_state_60,
                            });
                            format!(
                                "局部步 {step} · 旋转 {:?}° · 缩放 {:?} · 材质槽 {} RGB {:?} · 透明度 {}{}",
                                transform.rotation,
                                transform.scale,
                                value.group_entry,
                                rgb,
                                opacity,
                                match uv_offset {
                                    Some([u, v]) => format!(" · UV ({u:.3}, {v:.3})"),
                                    None if value.uv.repetitions != 0 && value.uv.mode != 0 =>
                                        format!(
                                            " · UV 模式 {} 依赖装备状态，尚未预览",
                                            value.uv.mode
                                        ),
                                    None => String::new(),
                                }
                            )
                        }
                    }
                };
                snapshots.push(snapshot);
            }
            sample.bindings.push(BindingSnapshot {
                id: binding.id,
                name: binding.source.name(),
                definitions: snapshots,
            });
        }
        sample
    }
}

/// 10BBC330 mode 0 uses an independent counter modulo cycle_steps. Signed
/// periods preserve scroll direction; positive repeats stop at the cycle end.
/// Stateful modes need equipment inputs and are deliberately not approximated.
fn sample_uv(channel: &UvAnimation, step: u32) -> Option<[f32; 2]> {
    if channel.repetitions == 0 || channel.mode != 0 {
        return None;
    }
    let cycle = u32::from(channel.cycle_steps);
    let counter = if cycle == 0 {
        0
    } else if channel.repetitions > 0 && step / cycle >= channel.repetitions as u32 {
        cycle
    } else {
        step % cycle
    } as i32;
    Some([channel.u_period, channel.v_period].map(|period| {
        if period == 0 {
            0.0
        } else {
            (counter % i32::from(period)) as f32 / f32::from(period)
        }
    }))
}

fn channel_period(duration: u16, ping_pong: bool) -> u32 {
    match (duration, ping_pong) {
        (0, _) => 0,
        (_, true) => u32::from(duration) * 2,
        (_, false) => u32::from(duration) + 1,
    }
}

// Native RGB/opacity: one-way includes both endpoints (duration + 1), ping-pong
// has 2 * duration steps. Positive repetitions end; negative ones do not decrement.
fn channel_phase(step: u32, duration: u16, repetitions: i16, ping_pong: bool) -> f32 {
    let period = channel_period(duration, ping_pong);
    if period == 0 || repetitions == 0 {
        return 0.0;
    }
    let duration = u32::from(duration);
    if repetitions > 0 && step / period >= repetitions as u32 {
        return if ping_pong { 0.0 } else { 1.0 };
    }
    let phase = step % period;
    let phase = if ping_pong && phase > duration {
        period - phase
    } else {
        phase
    };
    phase as f32 / duration as f32
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::inspect::{expand, inspect};

    pub(crate) fn fixture() -> (ResourceRef, ResourceRef) {
        let mut bytes = vec![0; 5600];
        bytes[..4].copy_from_slice(dat::MAGIC);
        for (at, value) in [
            (4, dat::VERSION),
            (12, dat::HEADER_SIZE as u32),
            (0x10, 3100),
            (0x280, 3600),
            (0x284, 4000),
            (0x294, 4600),
            (0x298, 5000),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (at, value) in [(0x72, 2u16), (0x74, 3), (0x7c, 2), (0x7e, 3)] {
            bytes[3100 + at..3102 + at].copy_from_slice(&value.to_le_bytes());
        }
        let attachment = AttachmentGroup {
            part_code: 4,
            definition_ids: [2, 0, 65535, 0, 0, 0, 0, 0],
        };
        bytes[3618..3636].copy_from_slice(&attachment.to_bytes());
        let mut definition = AttachmentDefinition::parse(&[0; 128]).unwrap();
        definition.node_index = 1;
        definition.resource_id = 42;
        definition.set_local_position([2.0, 3.0, 4.0]);
        bytes[4256..4384].copy_from_slice(&definition.to_bytes());
        let binding = ModelEffectBinding {
            part_code: 3,
            weapon_class: 7,
            variant: 2,
            model_id: 44,
            definition_ids: [1, 0, 65535, 0, 0, 0, 0, 0],
        };
        bytes[4624..4648].copy_from_slice(&binding.to_bytes());
        let mut definition = ModelEffectDefinition::parse(&[0; 180]).unwrap();
        for channel in &mut definition.scale {
            channel.start_bits = 1.0f32.to_bits();
        }
        definition.node_index = 2;
        definition.draw_group = 1;
        definition.group_entry = 2;
        definition.start_delay = 10;
        definition.set_translation_delta([-0.0, 5.0, 10.0]);
        definition.color.start_rgb = [0, 30, 100];
        definition.color.end_rgb = [100, 230, 200];
        definition.color.duration = 10;
        definition.color.repetitions = 1;
        definition.opacity.start = 0;
        definition.opacity.end = 255;
        definition.opacity.duration = 10;
        definition.opacity.repetitions = 1;
        bytes[5180..5360].copy_from_slice(&definition.to_bytes());
        // Nonzero container origin tests image-relative resolution.
        let mut archive = vec![0; 32];
        for (at, value) in [(0, 1u32), (4, 32), (8, bytes.len() as u32)] {
            archive[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        archive.extend(bytes);
        let mut document = inspect("effects.bin", archive.into());
        let tables = [0, 2].map(|index| {
            document
                .nodes
                .iter()
                .position(|node| node.kind == Kind::DatTable(dat::DATA_TABLES.len() + index))
                .unwrap()
        });
        for table in tables {
            document = expand(&document, table).unwrap();
        }
        let nodes = tables.map(|table| document.nodes[table].children[1]);
        let document = Arc::new(document);
        (
            ResourceRef {
                document: document.clone(),
                node: nodes[0],
            },
            ResourceRef {
                document,
                node: nodes[1],
            },
        )
    }

    fn effects() -> Effects {
        let (attachment, model) = fixture();
        let mut effects = Effects {
            bindings: vec![
                Binding::read(attachment).unwrap(),
                Binding::read(model).unwrap(),
            ],
            target: Target {
                nodes: 4,
                material_counts: vec![1, 3],
            },
            ..Effects::default()
        };
        for index in 0..effects.bindings.len() {
            effects.trigger(effects.bindings[index].id, 0, 0.0).unwrap();
        }
        effects
    }

    #[test]
    fn deferred_bindings_resolve_distinct_id_namespaces_without_changing_bytes() {
        let (attachment, model) = fixture();
        let source = model.document.buffers[0].clone();
        assert!(model.document.nodes[model.node].deferred);
        let a = Binding::read(attachment).unwrap();
        let b = Binding::read(model.clone()).unwrap();
        assert_eq!(a.entries.len(), 1);
        assert_eq!(b.entries.len(), 1);
        assert_eq!((a.entries[0].id, a.entries[0].definition.node()), (2, 1));
        assert_eq!((b.entries[0].id, b.entries[0].definition.node()), (1, 2));
        assert_eq!(
            b.entries[0].definition.position()[0].to_bits(),
            (-0.0f32).to_bits()
        );
        let expanded = expand(&model.document, model.node).unwrap();
        assert!(Arc::ptr_eq(&source, &expanded.buffers[0]));
        assert_eq!(&*source, &*model.document.buffers[0]);
        assert!(!is_binding(Kind::DatRecord(dat::DATA_TABLES.len() + 1)));
        assert!(!is_binding(Kind::DatTable(dat::DATA_TABLES.len())));
    }

    #[test]
    fn invalid_ids_and_empty_bindings_are_reported_without_replacing_sources() {
        let (_, model) = fixture();
        for id in [0u16, 65535] {
            let mut document = (*model.document).clone();
            let mut bytes = document.buffers[0].to_vec();
            let at = document.nodes[model.node].range.start + 8;
            bytes[at..at + 2].copy_from_slice(&id.to_le_bytes());
            document.buffers[0] = bytes.into();
            let bad = ResourceRef {
                document: Arc::new(document),
                node: model.node,
            };
            assert!(Binding::read(bad).is_err());
        }
        assert!(Binding::read(model).is_ok());
    }

    #[test]
    fn delay_seek_disable_and_multiple_bindings_are_deterministic() {
        let mut effects = effects();
        assert!(effects.sample(9.0).transforms.is_empty());
        let at_start = effects.sample(10.0);
        assert_eq!(at_start.transforms[0].node, 2);
        assert_eq!(at_start.transforms[0].translation, [-0.0, 5.0, 10.0]);
        assert_eq!(at_start.materials[0].opacity, 0.0);
        let middle = effects.sample(15.0);
        assert_eq!(
            middle.materials[0].rgb,
            [50.0 / 255.0, 130.0 / 255.0, 150.0 / 255.0]
        );
        assert_eq!(middle.materials[0].opacity, 127.0 / 255.0);
        assert_eq!(effects.sample(30.0).materials[0].opacity, 1.0);
        assert_eq!(effects.sample(10.0).materials[0].opacity, 0.0);
        effects.stop(effects.bindings[1].id, 0).unwrap();
        let disabled = effects.sample(30.0);
        assert!(disabled.transforms.is_empty());
        assert!(disabled.bindings[0].definitions[0].active);
        assert!(!disabled.bindings[1].definitions[0].active);
        assert!(self::effects().sample(30.0).bindings[1].definitions[0].active);
    }

    #[test]
    fn bad_targets_and_nonfinite_positions_never_override_a_valid_trigger() {
        let mut effects = effects();
        effects.target.material_counts[1] = 2;
        assert!(effects.sample(20.0).transforms.is_empty());
        effects.target.material_counts[1] = 3;
        effects.target.nodes = 2;
        assert!(effects.sample(20.0).transforms.is_empty());
        effects.target.nodes = 4;
        effects.bindings.push(effects.bindings[1].clone());
        effects.bindings[2].id += 10000;
        effects.trigger(effects.bindings[2].id, 0, 0.0).unwrap();
        let conflict = effects.sample(20.0);
        assert_eq!(conflict.transforms.len(), 1);
        assert!(
            conflict.bindings[1].definitions[0]
                .message
                .contains("最近触发")
        );
        assert!(conflict.bindings[2].definitions[0].active);
        effects.stop(effects.bindings[2].id, 0).unwrap();
        assert_eq!(effects.sample(20.0).transforms.len(), 1);
        let Definition::Model(value) = &mut effects.bindings[1].entries[0].definition else {
            panic!()
        };
        value.translation_delta_bits[0] = 0x7fc01234;
        assert!(effects.sample(20.0).transforms.is_empty());
        assert!(effects.sample(f32::NAN).materials.is_empty());
    }

    #[test]
    fn attachment_points_use_bone_orientation_and_unknown_modes_stay_explicit() {
        let mut effects = effects();
        let world = [
            0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 10.0, 20.0, 30.0, 1.0,
        ];
        let mut sample = effects.sample(0.0);
        sample.place(&[world; 4]);
        assert_eq!(
            sample.bindings[0].definitions[0].position,
            Some([7.0, 22.0, 34.0])
        );
        let Definition::Attachment(value) = &mut effects.bindings[0].entries[0].definition else {
            panic!()
        };
        value.attachment_mode = 4;
        assert!(!effects.sample(0.0).bindings[0].definitions[0].active);
    }

    #[test]
    fn color_and_opacity_phases_preserve_endpoints_and_repeat_signs() {
        assert_eq!(channel_phase(10, 10, -1, false), 1.0);
        assert_eq!(channel_phase(11, 10, -1, false), 0.0);
        assert_eq!(channel_phase(11, 10, 1, false), 1.0);
        assert_eq!(channel_phase(10, 10, -1, true), 1.0);
        assert_eq!(channel_phase(15, 10, -1, true), 0.5);
        assert_eq!(channel_phase(20, 10, -1, true), 0.0);
        assert_eq!(channel_phase(30, 10, 1, true), 0.0);
        assert_eq!(channel_phase(u32::MAX, 0, -1, true), 0.0);
    }

    #[test]
    fn rotation_scale_and_color_share_the_trigger_clock_and_do_not_modify_the_record() {
        let mut effects = effects();
        let Definition::Model(value) = &mut effects.bindings[1].entries[0].definition else {
            panic!()
        };
        value.rotation[2].start_degrees = -90;
        value.rotation[2].end_degrees = 90;
        value.rotation[2].repetitions = 1;
        value.rotation[2].duration = 10;
        value.scale[0].start_bits = 1.0f32.to_bits();
        value.scale[0].end_bits = 3.0f32.to_bits();
        value.scale[0].repetitions = 1;
        value.scale[0].duration = 10;
        let source = value.to_bytes();
        let middle = effects.sample(15.0);
        let transform = middle.transforms[0];
        assert_eq!(transform.mesh, 1);
        assert_eq!(transform.rotation, [0.0, 0.0, 0.0]);
        assert_eq!(transform.scale, [2.0, 1.0, 1.0]);
        assert_eq!(
            middle.materials[0].rgb,
            [50.0 / 255.0, 130.0 / 255.0, 150.0 / 255.0]
        );
        assert_eq!(effects.sample(20.0).transforms[0].rotation[2], 90.0);
        assert_eq!(effects.sample(10.0).transforms[0].rotation[2], -90.0);
        let Definition::Model(value) = &effects.bindings[1].entries[0].definition else {
            panic!()
        };
        assert_eq!(value.to_bytes(), source);
        effects.stop(effects.bindings[1].id, 0).unwrap();
        assert!(effects.sample(15.0).transforms.is_empty());
    }

    #[test]
    fn transform_uses_native_xyz_row_order_and_keeps_translation_independent() {
        let transform = NodeTransform {
            mesh: 1,
            node: 0,
            translation: [0.0, 5.0, 0.0],
            rotation: [0.0, 0.0, 90.0],
            scale: [2.0, 1.0, 0.0],
        };
        let mut matrix = [0.0; 16];
        for axis in 0..4 {
            matrix[axis * 5] = 1.0;
        }
        matrix[12..15].copy_from_slice(&[10.0, 20.0, 30.0]);
        transform.apply(&mut matrix);
        assert!(matrix[0].abs() < 0.00001);
        assert!((matrix[1] - 2.0).abs() < 0.00001);
        assert!((matrix[4] + 1.0).abs() < 0.00001);
        assert_eq!(matrix[10], 0.0);
        assert_eq!(matrix[12..15], [10.0, 25.0, 30.0]);
    }

    #[test]
    fn motion_keeps_looping_while_effects_advance_at_the_same_speed() {
        let effects = effects();
        for (speed, expected, alpha) in [
            (0.5, 15.0, 127.0 / 255.0),
            (1.0, 30.0, 1.0),
            (2.0, 60.0, 1.0),
        ] {
            for fps in [30, 60, 120] {
                let mut frame = 0.0;
                for _ in 0..fps {
                    frame = super::super::advance_frame(frame, 1.0 / fps as f32, speed);
                }
                assert!((frame - expected).abs() < 0.001);
                assert!(
                    (super::super::looping_motion_frame(frame, 12.0) - expected % 12.0).abs()
                        < 0.001
                );
                assert_eq!(effects.sample(frame).materials[0].opacity, alpha);
            }
        }
    }

    #[test]
    fn individual_definitions_can_be_selected_from_references_or_tables() {
        let (attachment, model) = fixture();
        for source in [attachment, model] {
            let document = expand(&source.document, source.node).unwrap();
            let reference = document.nodes[source.node].children[0];
            let selected = ResourceRef {
                document: Arc::new(document),
                node: reference,
            };
            assert!(is_definition(selected.kind()));
            let definition = Binding::read(selected.clone()).unwrap();
            let original = Binding::read(source).unwrap();
            assert_eq!(definition.entries.len(), 1);
            assert_eq!(definition.entries[0].id, original.entries[0].id);
            assert_eq!(
                definition.entries[0].definition.position(),
                original.entries[0].definition.position()
            );
            let independent = [
                Binding::read(selected.clone()).unwrap(),
                Binding::read(selected).unwrap(),
            ];
            assert_ne!(independent[0].id, independent[1].id);
        }
    }

    #[test]
    fn arbitrary_conditions_require_manual_triggers_and_latest_trigger_owns_the_target() {
        let (_, source) = fixture();
        let mut document = (*source.document).clone();
        let mut bytes = document.buffers[0].to_vec();
        let binding_at = document.nodes[source.node].range.start;
        bytes[binding_at + 10..binding_at + 12].copy_from_slice(&2u16.to_le_bytes());
        bytes[binding_at + 12..binding_at + 14].copy_from_slice(&0u16.to_le_bytes());
        // Unrelated raw condition IDs must never gate manual playback.
        for (id, condition) in [(1, 127), (2, 255)] {
            let mut value = ModelEffectDefinition::parse(&[0; 180]).unwrap();
            for channel in &mut value.scale {
                channel.start_bits = 1.0f32.to_bits();
            }
            value.activation_condition = condition;
            value.draw_group = 1;
            value.color.start_rgb = [255, 255, 255];
            value.color.repetitions = -1;
            value.opacity.start = 255;
            value.render_flags = 2;
            let at = 32 + 5000 + id * 180;
            bytes[at..at + 180].copy_from_slice(&value.to_bytes());
        }
        document.buffers[0] = bytes.into();
        let source = ResourceRef {
            document: Arc::new(document),
            node: source.node,
        };
        let mut effects = Effects {
            bindings: vec![Binding::read(source).unwrap()],
            target: Target {
                nodes: 3,
                material_counts: vec![1, 1],
            },
            ..Effects::default()
        };
        let id = effects.bindings[0].id;
        assert!(effects.sample(122.0).materials.is_empty());
        for slot in [1, 0, 1] {
            effects.trigger(id, slot, 122.0).unwrap();
            let sample = effects.sample(122.0);
            assert!(sample.bindings[0].definitions[slot].active);
            assert!(!sample.bindings[0].definitions[1 - slot].active);
            assert_eq!(sample.materials.len(), 1);
            assert_eq!(sample.materials[0].rgb, [1.0; 3]);
        }
        effects.stop(id, 1).unwrap();
        assert!(effects.sample(122.0).bindings[0].definitions[0].active);
        effects.stop(id, 0).unwrap();
        assert!(effects.sample(122.0).materials.is_empty());
    }

    #[test]
    fn replay_uses_a_local_origin_without_rewinding_the_shared_clock() {
        let mut effects = effects();
        let id = effects.bindings[1].id;
        effects.trigger(id, 0, 122.0).unwrap();
        assert!(effects.sample(121.0).materials.is_empty());
        assert!(effects.sample(131.0).materials.is_empty());
        assert_eq!(effects.sample(132.0).materials[0].opacity, 0.0);
        assert_eq!(effects.sample(137.0).materials[0].opacity, 127.0 / 255.0);
        effects.trigger(id, 0, 137.0).unwrap();
        assert!(effects.sample(137.0).materials.is_empty());
        assert_eq!(effects.sample(147.0).materials[0].opacity, 0.0);
        let end = effects.sample(147.0).bindings[1].definitions[0].frames;
        effects.stop(id, 0).unwrap();
        assert!(effects.sample(147.0).materials.is_empty());
        assert_eq!(effects.sample(147.0).bindings[1].definitions[0].frames, end);
        assert!(effects.trigger(id, 0, f32::NAN).is_err());
    }

    #[test]
    fn seeking_and_stepping_a_track_leave_other_origins_and_stopped_tracks_unchanged() {
        let mut effects = effects();
        // The fixture has a 10-step delay and a one-way 10-step animation.
        let binding = effects.bindings[1].id;
        effects.trigger(binding, 0, 1000.0).unwrap();
        let before = effects.sample(1015.0);
        let track = &before.bindings[1].definitions[0];
        assert_eq!(
            (track.frames, track.frame, track.looping),
            (21.0, Some(15.0), false)
        );
        let other_origin = effects.bindings[0].entries[0].started_at;
        effects.seek(binding, 0, 1015.0, 12.0).unwrap();
        let sought = effects.sample(1015.0);
        assert_eq!(sought.bindings[1].definitions[0].frame, Some(12.0));
        assert_eq!(sought.materials[0].opacity, 51.0 / 255.0);
        effects.step(binding, 0, 1015.0, 1).unwrap();
        assert_eq!(
            effects.sample(1015.0).bindings[1].definitions[0].frame,
            Some(13.0)
        );
        assert_eq!(effects.bindings[0].entries[0].started_at, other_origin);
        // Stepping back from a completed finite track starts at its visible end.
        effects.step(binding, 0, 5000.0, -1).unwrap();
        assert_eq!(
            effects.sample(5000.0).bindings[1].definitions[0].frame,
            Some(20.0)
        );
        assert!(effects.seek(binding, 0, 5000.0, f32::NAN).is_err());
        assert!(effects.seek(binding, 0, 5000.0, 22.0).is_err());
        effects.stop(binding, 0).unwrap();
        assert!(effects.seek(binding, 0, 5000.0, 12.0).is_err());
        assert!(effects.step(binding, 0, 5000.0, 1).is_err());
        let stopped = effects.sample(5000.0);
        assert_eq!(stopped.bindings[1].definitions[0].frame, None);
        assert_eq!(stopped.bindings[1].definitions[0].frames, 21.0);
    }

    #[test]
    fn looping_track_omits_repeat_delay_and_seeking_preserves_trigger_precedence() {
        let mut effects = effects();
        let binding = &mut effects.bindings[1];
        let Definition::Model(value) = &mut binding.entries[0].definition else {
            unreachable!()
        };
        value.color.duration = 6;
        value.color.repetitions = 3;
        value.opacity.duration = 10;
        value.opacity.ping_pong = 1;
        value.opacity.repetitions = -1;
        // Finite color length is 3 * 7; opacity's displayed cycle is 20.
        let mut second = binding.entries[0].clone();
        second.slot = 1;
        binding.entries.push(second);
        let id = binding.id;
        effects.trigger(id, 1, 100.0).unwrap();
        effects.trigger(id, 0, 102.0).unwrap();
        let sample = effects.sample(138.0);
        let track = &sample.bindings[1].definitions[0];
        assert_eq!(track.frames, 31.0);
        assert_eq!(track.frame, Some(15.0));
        assert!(track.active && track.looping);
        // Only display wraps by the longest span; each real channel keeps its own cycle.
        assert_eq!(sample.materials[0].opacity, 153.0 / 255.0);
        let other_origin = effects.bindings[1].entries[1].started_at;
        effects.seek(id, 0, 138.0, 15.0).unwrap();
        let sought = effects.sample(138.0);
        assert!(sought.bindings[1].definitions[0].active);
        assert!(!sought.bindings[1].definitions[1].active);
        assert_eq!(effects.bindings[1].entries[1].started_at, other_origin);
    }

    #[test]
    fn uv_scroll_preserves_signed_periods_cycle_boundaries_and_finite_repeats() {
        let mut channel = UvAnimation {
            u_period: 8,
            v_period: -10,
            cycle_steps: 30,
            repetitions: -1,
            state_step_bits: 0,
            mode: 0,
        };
        assert_eq!(sample_uv(&channel, 5), Some([0.625, -0.5]));
        assert_eq!(sample_uv(&channel, 29), Some([0.625, -0.9]));
        assert_eq!(sample_uv(&channel, 30), Some([0.0, 0.0]));
        assert_eq!(sample_uv(&channel, 3005), Some([0.625, -0.5]));
        channel.repetitions = 1;
        assert_eq!(sample_uv(&channel, 30), Some([0.75, 0.0]));
        assert_eq!(sample_uv(&channel, 3005), Some([0.75, 0.0]));
        channel.cycle_steps = 0;
        assert_eq!(sample_uv(&channel, u32::MAX), Some([0.0; 2]));
        channel.repetitions = 0;
        assert_eq!(sample_uv(&channel, 5), None);
        channel.repetitions = -1;
        channel.mode = 4;
        assert_eq!(sample_uv(&channel, 5), None);
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads the user's DAT 819 sample"]
    fn original_dat_819_for_model_4521_scrolls_uv_on_both_branches() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let bytes = std::fs::read(root.join("dat/mhfdat.bin")).unwrap();
        let document = inspect("mhfdat.bin", bytes.into());
        let table = document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::DatTable(dat::DATA_TABLES.len() + 2))
            .unwrap();
        let document = expand(&document, table).unwrap();
        let node = document.nodes[table].children[819];
        let source = ResourceRef {
            document: Arc::new(document),
            node,
        };
        let original = source.bytes().unwrap().to_vec();
        let record = ModelEffectBinding::parse(&original).unwrap();
        assert_eq!(record.model_id, 4521);
        let mut effects = Effects {
            bindings: vec![Binding::read(source.clone()).unwrap()],
            target: Target {
                nodes: 1,
                material_counts: vec![1, 1],
            },
            ..Effects::default()
        };
        assert_eq!(
            effects.bindings[0]
                .entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            [391, 415]
        );
        effects.trigger(effects.bindings[0].id, 0, 122.0).unwrap();
        let sample = effects.sample(122.0);
        assert_eq!(sample.materials.len(), 1);
        assert_eq!(sample.materials[0].rgb, [1.0; 3]);
        assert_eq!(sample.materials[0].opacity, 1.0);
        assert_eq!(sample.materials[0].render_flags, 2);
        assert!(sample.bindings[0].definitions[0].active);
        assert!(!sample.bindings[0].definitions[1].active);
        assert!(
            sample.bindings[0]
                .definitions
                .iter()
                .all(|entry| entry.frames == 30.0 && entry.looping)
        );
        for slot in 0..2 {
            effects
                .trigger(effects.bindings[0].id, slot, 122.0)
                .unwrap();
            for (local, expected_v) in [
                (0, 0.0),
                (5, -0.5),
                (9, -0.9),
                (10, 0.0),
                (35, -0.5),
                (3005, -0.5),
            ] {
                let sample = effects.sample(122.0 + local as f32);
                assert_eq!(sample.materials.len(), 1);
                let material = &sample.materials[0];
                assert_eq!((material.mesh, material.entry), (1, 0));
                assert_eq!(material.uv_offset, Some([0.0, expected_v]));
                assert_eq!(
                    material.opacity,
                    if slot == 0 || local == 0 { 1.0 } else { 0.0 }
                );
            }
        }
        assert_eq!(source.bytes().unwrap(), original);
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; samples original color and transform definitions"]
    fn original_dat_color_and_transform_definitions_animate_without_source_changes() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let document = inspect(
            "mhfdat.bin",
            std::fs::read(root.join("dat/mhfdat.bin")).unwrap().into(),
        );
        let table = document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::DatTable(dat::DATA_TABLES.len() + 3))
            .unwrap();
        let document = Arc::new(expand(&document, table).unwrap());
        for id in [1, 12] {
            let source = ResourceRef {
                document: document.clone(),
                node: document.nodes[table].children[id],
            };
            let original = source.bytes().unwrap().to_vec();
            let mut effects = Effects {
                bindings: vec![Binding::read(source.clone()).unwrap()],
                target: Target {
                    nodes: 1,
                    material_counts: vec![1, 1],
                },
                ..Default::default()
            };
            let delay = effects.bindings[0].entries[0].definition.delay();
            effects.trigger(effects.bindings[0].id, 0, 0.0).unwrap();
            let start = effects.sample(f32::from(delay));
            let middle = effects.sample(f32::from(delay) + if id == 1 { 40.0 } else { 15.0 });
            assert!(start.bindings[0].definitions[0].active);
            assert!(middle.bindings[0].definitions[0].active);
            if id == 1 {
                assert_eq!(start.materials[0].rgb, [1.0, 1.0, 50.0 / 255.0]);
                assert_ne!(start.materials[0].rgb, middle.materials[0].rgb);
            } else {
                assert_eq!(start.transforms[0].scale, [0.0; 3]);
                assert_eq!(middle.transforms[0].scale, [0.5; 3]);
                assert_eq!(middle.transforms[0].rotation, [0.0, 0.0, 180.0]);
            }
            assert_eq!(source.bytes().unwrap(), original);
        }
    }
}
