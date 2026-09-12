//! Owned MOT compilation and binding to a workbench resource's skeleton.
//!
//! 100018B0 creates a linked native allocation. The validated child-before-sibling
//! order replaces 10008CC0's recursive binding. Sampling keeps the native curve
//! evaluator and local-matrix builder but runs one node at a time, avoiding the
//! recursive 100083D0 walk. No hunter motion banks are involved.

use super::{Client, get, put};
use crate::preview::ResourceRef;
use mhf_resource::motion::{KeyEncoding, Keyframe, Motion};
use std::{collections::BTreeMap, mem::transmute, slice};
use windows::Win32::System::Threading::{
    CRITICAL_SECTION, EnterCriticalSection, LeaveCriticalSection,
};

const NODE_SIZE: usize = 448;
const NODE_COUNT: usize = 194;
const NODE_TAG: usize = 198;
const NODE_SIBLING: usize = 204;
const NODE_CHILD: usize = 208;
const NODE_MOTION: usize = 212;
const NODE_TRACK_OFFSET: usize = 216;
const ALLOCATION_HEAD: usize = 0x1e73_d370;
const ALLOCATION_LOCK: usize = 0x1e73_acf8;

const SIGNATURES: &[(usize, &[u8])] = &[
    (
        0x1000_1790,
        &[
            0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x8b, 0x41, 0x04, 0x53, 0x57, 0x33,
        ],
    ),
    (
        0x1000_18b0,
        &[
            0x55, 0x8b, 0xec, 0x83, 0xec, 0x30, 0x53, 0x8b, 0x5d, 0x08, 0x56, 0x57, 0x8b, 0xcb,
        ],
    ),
    (
        0x1000_9ed0,
        &[
            0x55, 0x8b, 0xec, 0x51, 0x8b, 0x4d, 0x08, 0x0f, 0xb7, 0x01, 0x25, 0x00, 0xf0, 0x00,
            0x00, 0x53,
        ],
    ),
    (
        0x1000_9a20,
        &[
            0x53, 0x8b, 0xdc, 0x83, 0xec, 0x08, 0x83, 0xe4, 0xf0, 0x83, 0xc4, 0x04, 0x55, 0x8b,
            0x6b, 0x04,
        ],
    ),
    (
        0x1000_1000,
        &[0x56, 0x57, 0x8b, 0x78, 0xf0, 0x8d, 0x70, 0xf0],
    ),
];

/// # Safety
/// The supported native DLL must be mapped and retained by the calling Mod.
pub(super) unsafe fn validate(client: Client) -> Result<(), String> {
    for &(address, expected) in SIGNATURES {
        if unsafe { slice::from_raw_parts(client.address(address) as *const u8, expected.len()) }
            != expected
        {
            return Err(format!("不支持此客户端的 MOT 编译或绑定接口：{address:#x}"));
        }
    }
    Ok(())
}

struct MotionPlan {
    compiled_bytes: usize,
    frames: f32,
}

impl MotionPlan {
    fn read(motion: &Motion<'_>) -> Result<Self, String> {
        // 100018B0 tests the first byte, not the entire kind DWORD.
        if !matches!(motion.header.kind as u8, 1 | 2) {
            return Err("此 MOT 轨道类型尚未确认可由原生编译器处理".into());
        }
        if motion.tracks.is_empty() || motion.tracks.len() > i16::MAX as usize {
            return Err("MOT 轨道数量超出原生有符号 WORD 范围".into());
        }
        // 10001790: 32-byte header + DWORD track offsets; then each track's
        // 8-byte header, 8-byte channel descriptors, and encoded key data.
        let mut compiled_bytes = 32 + 4 * motion.tracks.len();
        let mut frames = 0.0_f32;
        for (track_index, track) in motion.tracks.iter().enumerate() {
            if track.channels.len() > i16::MAX as usize {
                return Err(format!("轨道 {track_index} 的通道数量超出原生范围"));
            }
            compiled_bytes = compiled_bytes
                .checked_add(8 + 8 * track.channels.len())
                .ok_or("MOT 编译缓冲区长度溢出")?;
            for (channel_index, channel) in track.channels.iter().enumerate() {
                // The size query reads all 32 count bits, even for disabled
                // channels. Native copy functions later use only the low WORD.
                let storage_stride = match (channel.header.kind >> 16) as u8 {
                    0x11 => 4,
                    0x12 | 0x21 => 8,
                    0x13 => 12,
                    0x22 => 16,
                    0x23 => 20,
                    _ => 0,
                };
                let storage_bytes = (channel.header.count as usize)
                    .checked_mul(storage_stride)
                    .ok_or("MOT 关键帧缓冲区长度溢出")?;
                compiled_bytes = compiled_bytes
                    .checked_add(storage_bytes)
                    .filter(|&size| size <= i32::MAX as usize - 16)
                    .ok_or("MOT 编译缓冲区超出原生有符号长度范围")?;
                match channel.encoding() {
                    KeyEncoding::Disabled => continue,
                    KeyEncoding::Unknown(_) => {
                        return Err(format!(
                            "轨道 {track_index} 通道 {channel_index} 的关键帧编码尚未支持原生预览"
                        ));
                    }
                    _ => {}
                }
                if channel.header.count == 0 || channel.header.count > i16::MAX as u32 {
                    return Err(format!(
                        "轨道 {track_index} 通道 {channel_index} 的关键帧数量不能安全用于原生采样"
                    ));
                }
                if channel.target_slot().is_none() {
                    return Err(format!(
                        "轨道 {track_index} 通道 {channel_index} 未指定唯一的原生变换分量"
                    ));
                }
                let mut previous_frame = None;
                for key_index in 0..usize::from(channel.native_key_count()) {
                    let key = channel.key(key_index).map_err(|error| error.to_string())?;
                    let frame = key.frame();
                    if !frame.is_finite() || previous_frame.is_some_and(|previous| frame < previous)
                    {
                        return Err(format!(
                            "轨道 {track_index} 通道 {channel_index} 的帧坐标必须有限且非递减，原始数据未修改"
                        ));
                    }
                    if !supported_key_values(key) {
                        return Err(format!(
                            "轨道 {track_index} 通道 {channel_index} 包含非有限值或尚未确认的插值类型"
                        ));
                    }
                    previous_frame = Some(frame);
                }
                // 100018B0 takes the largest final key frame, starting at zero.
                // Negative pre-roll keys remain valid and unchanged.
                if let Some(last) = previous_frame
                    && last > frames
                {
                    frames = last;
                }
            }
        }
        Ok(Self {
            compiled_bytes,
            frames,
        })
    }
}

fn supported_key_values(key: Keyframe) -> bool {
    match key {
        Keyframe::I16Pair { .. } | Keyframe::I16Quad { .. } => true,
        Keyframe::Mixed12 { unknown_00, .. } => matches!(unknown_00, 0x10000 | 0x20000),
        Keyframe::F32Pair { value_bits, .. } => f32::from_bits(value_bits).is_finite(),
        Keyframe::F32Quad {
            value_bits,
            parameter_bits,
            ..
        } => {
            f32::from_bits(value_bits).is_finite()
                && parameter_bits
                    .into_iter()
                    .all(|bits| f32::from_bits(bits).is_finite())
        }
        Keyframe::F32Five {
            unknown_00,
            value_bits,
            parameter_bits,
            ..
        } => {
            matches!(unknown_00, 0x10000 | 0x20000)
                && f32::from_bits(value_bits).is_finite()
                && parameter_bits
                    .into_iter()
                    .all(|bits| f32::from_bits(bits).is_finite())
        }
    }
}

#[derive(Clone, Copy)]
struct BindingTarget {
    address: usize,
    previous_motion: usize,
    previous_track_offset: u32,
}

struct BindingPlan {
    /// Exactly the child-before-sibling order consumed by 10008CC0.
    targets: Vec<BindingTarget>,
}

struct BindingNode {
    tag: u16,
    sibling: Option<usize>,
    child: Option<usize>,
}

impl BindingPlan {
    unsafe fn read(
        roots: &[usize],
        node_range: (usize, usize),
        tracks: usize,
    ) -> Result<Self, String> {
        let (nodes, count) = node_range;
        if roots.is_empty() || nodes == 0 || count == 0 || count > i16::MAX as usize {
            return Err("MOT 绑定所需的原生骨架范围无效".into());
        }
        nodes
            .checked_add(count.checked_mul(NODE_SIZE).ok_or("骨架范围长度溢出")?)
            .ok_or("骨架地址范围溢出")?;
        let mut links = Vec::with_capacity(count);
        for index in 0..count {
            let node = nodes + index * NODE_SIZE;
            links.push(BindingNode {
                tag: unsafe { get(node + NODE_TAG) },
                sibling: node_index(nodes, count, unsafe { get(node + NODE_SIBLING) })?,
                child: node_index(nodes, count, unsafe { get(node + NODE_CHILD) })?,
            });
        }
        // Validate the complete forest before selecting a motion group. A bad
        // link in an unbound attachment must not escape the ownership check.
        let mut seen = vec![false; count];
        let mut forests = Vec::with_capacity(roots.len());
        for &root in roots {
            let root_index = node_index(nodes, count, root)?.ok_or("骨架根节点为空")?;
            let mut pending = vec![root_index];
            let mut entries = BTreeMap::new();
            let mut visited = 0;
            while let Some(index) = pending.pop() {
                if seen[index] {
                    return Err("骨架根、子节点或兄弟链重复或成环".into());
                }
                seen[index] = true;
                visited += 1;
                // 10008D30 selects the first matching WORD tag in the same
                // child-before-sibling order used to validate this root.
                entries.entry(links[index].tag).or_insert(index);
                if let Some(sibling) = links[index].sibling {
                    pending.push(sibling);
                }
                if let Some(child) = links[index].child {
                    pending.push(child);
                }
            }
            if unsafe { get::<u16>(root + NODE_COUNT) } as usize != visited {
                return Err("原生根节点计数与其实际遍历范围不一致".into());
            }
            forests.push(entries);
        }
        if seen.iter().any(|&visited| !visited) {
            return Err("原生骨架存在未归属任何根的节点".into());
        }

        let mut candidates = Vec::new();
        let mut available = Vec::new();
        for (root, entries) in forests.into_iter().enumerate() {
            for (tag, entry) in entries {
                let mut pending = vec![entry];
                let mut indices = Vec::new();
                while let Some(index) = pending.pop() {
                    let node = &links[index];
                    // 10008CC0 always follows siblings, but consumes a track
                    // and descends into children only when the tag matches.
                    if let Some(sibling) = node.sibling {
                        pending.push(sibling);
                    }
                    if node.tag == tag {
                        indices.push(index);
                        if let Some(child) = node.child {
                            pending.push(child);
                        }
                    }
                }
                available.push(format!("根 {root} / tag {tag}：{} 条", indices.len()));
                if indices.len() == tracks {
                    candidates.push((available.len() - 1, indices));
                }
            }
        }
        if candidates.len() != 1 {
            return Err(if candidates.is_empty() {
                format!(
                    "MOT 的 {tracks} 条轨道不匹配任何完整动画分组（{}）",
                    available.join("，")
                )
            } else {
                format!(
                    "MOT 的 {tracks} 条轨道匹配多个动画分组，无法唯一绑定（{}）",
                    candidates
                        .iter()
                        .map(|&(label, _)| available[label].as_str())
                        .collect::<Vec<_>>()
                        .join("，")
                )
            });
        }
        let (_, indices) = candidates.pop().unwrap();
        let targets = indices
            .into_iter()
            .map(|index| {
                let address = nodes + index * NODE_SIZE;
                BindingTarget {
                    address,
                    previous_motion: unsafe { get(address + NODE_MOTION) },
                    previous_track_offset: unsafe { get(address + NODE_TRACK_OFFSET) },
                }
            })
            .collect();
        Ok(Self { targets })
    }

    unsafe fn bind(&self, compiled: usize, offsets: &[u32]) -> Result<(), String> {
        if compiled == 0 || offsets.len() != self.targets.len() {
            return Err("MOT 编译结果不能精确对应已验证的骨架节点".into());
        }
        // 10008CC0 writes only these two fields for each matching tag. The plan
        // already checked every tag, link, node and track; no native recursion
        // or temporary hierarchy mutation is necessary.
        for (target, &offset) in self.targets.iter().zip(offsets) {
            unsafe {
                put(target.address + NODE_MOTION, compiled);
                put(target.address + NODE_TRACK_OFFSET, offset);
            }
        }
        Ok(())
    }

    unsafe fn restore(&self) {
        for target in &self.targets {
            unsafe {
                put(target.address + NODE_MOTION, target.previous_motion);
                put(
                    target.address + NODE_TRACK_OFFSET,
                    target.previous_track_offset,
                );
            }
        }
    }
}

fn node_index(nodes: usize, count: usize, pointer: usize) -> Result<Option<usize>, String> {
    if pointer == 0 {
        return Ok(None);
    }
    let offset = pointer
        .checked_sub(nodes)
        .ok_or("骨骼链接超出当前资源范围")?;
    if !offset.is_multiple_of(NODE_SIZE) || offset / NODE_SIZE >= count {
        return Err("骨骼链接未指向当前资源内的完整节点".into());
    }
    Ok(Some(offset / NODE_SIZE))
}

/// Validate the compiler's relative track/channel/key ranges before a sampler
/// can follow them. Offsets retain their native order and units.
fn compiled_offsets(
    bytes: &[u8],
    motion: &Motion<'_>,
    plan: &MotionPlan,
) -> Result<Vec<u32>, String> {
    if bytes.len() != plan.compiled_bytes || bytes.len() < 32 {
        return Err("原生 MOT 编译长度与已验证的输入布局不一致".into());
    }
    let count = u16::from_le_bytes(bytes[2..4].try_into().unwrap()) as usize;
    let first = f32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let last = f32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let table = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let size = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
    if count != motion.tracks.len()
        || size != bytes.len()
        || table != 32
        || !first.is_finite()
        || !last.is_finite()
        || last < 0.0
        || last.to_bits() != plan.frames.to_bits()
    {
        return Err("原生 MOT 编译头的轨道数、帧范围或偏移无效".into());
    }
    let table_bytes = bytes
        .get(table..table + count * 4)
        .ok_or("原生 MOT 轨道表越界")?;
    let mut offsets = Vec::with_capacity(count);
    for (index, raw_offset) in table_bytes.as_chunks::<4>().0.iter().enumerate() {
        let offset = u32::from_le_bytes(*raw_offset);
        let at = offset as usize;
        if at < table + count * 4 {
            return Err("原生 MOT 轨道覆盖其头或偏移表".into());
        }
        let header = bytes
            .get(at..at.checked_add(8).ok_or("原生 MOT 轨道偏移溢出")?)
            .ok_or("原生 MOT 轨道头越界")?;
        let kind = u16::from_le_bytes(header[..2].try_into().unwrap());
        let channels = u16::from_le_bytes(header[2..4].try_into().unwrap()) as usize;
        let channel_table = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let track = &motion.tracks[index];
        if kind != u16::from(motion.header.kind as u8) << 12 || channels != track.channels.len() {
            return Err("原生 MOT 轨道类型或通道数量与源数据不一致".into());
        }
        let channel_end = channel_table
            .checked_add(channels * 8)
            .ok_or("原生 MOT 通道表长度溢出")?;
        let descriptors = bytes
            .get(channel_table..channel_end)
            .ok_or("原生 MOT 通道表越界")?;
        for (record, source) in descriptors.as_chunks::<8>().0.iter().zip(&track.channels) {
            if source.encoding() == KeyEncoding::Disabled {
                if record[0] != 0x20 || u16::from_le_bytes(record[2..4].try_into().unwrap()) != 0 {
                    return Err("禁用的 MOT 通道被意外转换为有效关键帧".into());
                }
                continue;
            }
            let native_kind = (source.header.kind >> 16) as u8;
            let keys = u16::from_le_bytes(record[2..4].try_into().unwrap());
            if record[0] != native_kind
                || Some(record[1]) != source.target_slot()
                || keys != source.native_key_count()
            {
                return Err("原生 MOT 通道编码、目标分量或关键帧数量与源数据不一致".into());
            }
            let key_offset = u32::from_le_bytes(record[4..8].try_into().unwrap()) as usize;
            let key_length =
                usize::from(keys) * source.encoding().stride().ok_or("未知 MOT 关键帧布局")?;
            let key_end = key_offset
                .checked_add(key_length)
                .ok_or("原生 MOT 关键帧范围溢出")?;
            if key_offset < table + count * 4 || bytes.get(key_offset..key_end).is_none() {
                return Err("原生 MOT 关键帧数据越界".into());
            }
        }
        offsets.push(offset);
    }
    Ok(offsets)
}

struct AllocationListLock(*mut CRITICAL_SECTION);

impl AllocationListLock {
    unsafe fn acquire(client: Client) -> Self {
        let critical = client.address(ALLOCATION_LOCK) as *mut CRITICAL_SECTION;
        unsafe {
            EnterCriticalSection(critical);
        }
        Self(critical)
    }

    unsafe fn contains(&self, client: Client, payload: usize) -> Result<bool, String> {
        let wanted = payload.checked_sub(16).ok_or("原生 MOT 分配地址无效")?;
        let mut current = unsafe { client.read::<usize>(ALLOCATION_HEAD) };
        let mut previous = 0;
        while current != 0 {
            if !current.is_multiple_of(4)
                || current.checked_add(16).is_none()
                || unsafe { get::<usize>(current + 4) } != previous
            {
                return Err("原生分配链表不一致，未释放 MOT 缓冲区".into());
            }
            if current == wanted {
                return Ok(true);
            }
            previous = current;
            current = unsafe { get(current) };
        }
        Ok(false)
    }
}

impl Drop for AllocationListLock {
    fn drop(&mut self) {
        unsafe {
            LeaveCriticalSection(self.0);
        }
    }
}

unsafe fn free_compiled(client: Client, compiled: usize) -> Result<(), String> {
    let locked = unsafe { AllocationListLock::acquire(client) };
    if unsafe { locked.contains(client, compiled) }? {
        // Windows critical sections are recursive. Keep ownership checking and
        // the native unlink/free atomic with respect to native allocations.
        unsafe {
            release_native(client.address(0x1000_1000), compiled);
        }
    }
    Ok(())
}

#[must_use = "release on the task thread before releasing the owning skeleton"]
pub(super) struct NativeMotion {
    pub(super) source: ResourceRef,
    compiled: Option<usize>,
    targets: Vec<usize>,
    offsets: Vec<u32>,
}

impl NativeMotion {
    pub(super) fn duration(source: &ResourceRef) -> Result<f32, String> {
        let motion = Motion::parse(source.bytes()?).map_err(|error| error.to_string())?;
        Ok(MotionPlan::read(&motion)?.frames)
    }

    /// Inspect a prospective binding without compiling or changing node pointers.
    ///
    /// # Safety
    /// The roots and range must belong to a live, idle owned skeleton.
    pub(super) unsafe fn binding_nodes(
        source: &ResourceRef,
        roots: &[usize],
        node_range: (usize, usize),
    ) -> Result<Vec<usize>, String> {
        let motion = Motion::parse(source.bytes()?).map_err(|error| error.to_string())?;
        let binding = unsafe { BindingPlan::read(roots, node_range, motion.tracks.len()) }?;
        Ok(binding
            .targets
            .into_iter()
            .map(|target| target.address)
            .collect())
    }

    pub(super) fn target_nodes(&self) -> impl Iterator<Item = usize> + '_ {
        self.targets.iter().copied()
    }

    /// # Safety
    /// The retained compilation and every target skeleton node must still be live.
    pub(super) unsafe fn activate(&self) -> Result<(), String> {
        let compiled = self.compiled.ok_or("动画编译资源已释放")?;
        if self.offsets.len() != self.targets.len() {
            return Err("动画轨道和绑定节点数量不一致".into());
        }
        for (&node, &offset) in self.targets.iter().zip(&self.offsets) {
            unsafe {
                put(node + NODE_MOTION, compiled);
                put(node + NODE_TRACK_OFFSET, offset);
            }
        }
        Ok(())
    }

    /// # Safety
    /// Roots/range must describe the live owning skeleton on the task thread
    /// with sampling and rendering stopped. Release this motion before freeing
    /// that skeleton or unloading the game DLL.
    pub(super) unsafe fn load(
        client: Client,
        source: ResourceRef,
        roots: &[usize],
        node_range: (usize, usize),
    ) -> Result<Self, String> {
        let motion = Motion::parse(source.bytes()?).map_err(|error| error.to_string())?;
        let plan = MotionPlan::read(&motion)?;
        let binding = unsafe { BindingPlan::read(roots, node_range, motion.tracks.len()) }?;
        let native_size: unsafe extern "thiscall" fn(*const u8) -> i32 =
            unsafe { transmute(client.address(0x1000_1790)) };
        if unsafe { native_size(motion.as_bytes().as_ptr()) } != plan.compiled_bytes as i32 {
            return Err("原生 MOT 分配长度与类型解析结果不一致，未更改骨架绑定".into());
        }
        let compile: unsafe extern "C" fn(*const u8) -> usize =
            unsafe { transmute(client.address(0x1000_18b0)) };
        let compiled = unsafe { compile(motion.as_bytes().as_ptr()) };
        if compiled == 0 {
            return Err("原生 MOT 编译分配失败".into());
        }
        let mut bound = false;
        let result = (|| {
            let locked = unsafe { AllocationListLock::acquire(client) };
            if !unsafe { locked.contains(client, compiled) }? {
                return Err("原生 MOT 编译结果未注册到分配链表".into());
            }
            let compiled_bytes =
                unsafe { slice::from_raw_parts(compiled as *const u8, plan.compiled_bytes) };
            let offsets = compiled_offsets(compiled_bytes, &motion, &plan)?;
            drop(locked);
            unsafe {
                binding.bind(compiled, &offsets)?;
            }
            bound = true;
            for (target, &offset) in binding.targets.iter().zip(&offsets) {
                if unsafe { get::<usize>(target.address + NODE_MOTION) } != compiled
                    || unsafe { get::<u32>(target.address + NODE_TRACK_OFFSET) } != offset
                {
                    return Err("MOT 未按已验证的节点顺序完成绑定".into());
                }
            }
            Ok(offsets)
        })();
        let offsets = match result {
            Ok(offsets) => offsets,
            Err(error) => {
                if bound {
                    unsafe {
                        binding.restore();
                    }
                }
                if let Err(cleanup) = unsafe { free_compiled(client, compiled) } {
                    return Err(format!("{error}；{cleanup}"));
                }
                return Err(error);
            }
        };
        Ok(Self {
            source,
            compiled: Some(compiled),
            targets: binding
                .targets
                .into_iter()
                .map(|target| target.address)
                .collect(),
            offsets,
        })
    }

    /// # Safety
    /// Task thread after sampling/drawing stops, before releasing the skeleton.
    /// If native teardown already removed the allocation, no stale allocation
    /// or node pointer is dereferenced.
    pub(super) unsafe fn release(&mut self, client: Client) -> Result<(), String> {
        let Some(compiled) = self.compiled else {
            return Ok(());
        };
        let locked = unsafe { AllocationListLock::acquire(client) };
        if unsafe { locked.contains(client, compiled) }? {
            for &node in &self.targets {
                // Releasing an older clip must not erase a replacement binding.
                if unsafe { get::<usize>(node + NODE_MOTION) } == compiled {
                    unsafe {
                        put(node + NODE_MOTION, 0usize);
                        put(node + NODE_TRACK_OFFSET, 0u32);
                    }
                }
            }
            unsafe {
                release_native(client.address(0x1000_1000), compiled);
            }
        }
        self.compiled = None;
        Ok(())
    }
}

/// Evaluate one node, without following its parent/child/sibling pointers.
///
/// # Safety
/// `node` must address a validated writable 448-byte node in the owning live
/// skeleton, on its world-render thread with no concurrent release/binding.
/// Its motion binding, if nonzero, must be a validated retained compiled MOT.
/// `frame` must be finite, and both helper signatures must have been verified.
pub(super) unsafe fn sample(client: Client, node: usize, frame: f32) {
    unsafe {
        sample_with(
            client.address(0x1000_9ed0),
            client.address(0x1000_9a20),
            node,
            frame,
        );
    }
}

unsafe fn sample_with(evaluate: usize, matrix: usize, node: usize, frame: f32) {
    unsafe {
        // 100083D0 copies ten floats, including the final preserved component.
        // Copy their encoded values directly; the helpers build the same S*R
        // local matrix and translation as the native per-node path.
        let bind_values: [u8; 40] = get(node + 220);
        put(node + 260, bind_values);
        sample_native(evaluate, matrix, node, frame);
    }
}

// 10009ED0: EDI=compiled base, caller-clean stack=(track, values, frame, cache).
// 10009A20: ESI=values, EDI=local matrix; no stack arguments. These are the
// exact helpers called by 100083D0 before its recursive child/sibling calls.
#[unsafe(naked)]
unsafe extern "C" fn sample_native(_evaluate: usize, _matrix: usize, _node: usize, _frame: f32) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push ebx",
        "push esi",
        "push edi",
        "mov ebx, [ebp + 16]",
        "lea esi, [ebx + 260]",
        "mov edi, [ebx + 212]",
        "test edi, edi",
        "jz 2f",
        "mov eax, [ebx + 216]",
        "add eax, edi",
        "lea ecx, [ebx + 348]",
        "push ecx",
        "push dword ptr [ebp + 20]",
        "push esi",
        "push eax",
        "call dword ptr [ebp + 8]",
        "add esp, 16",
        "2:",
        "lea edi, [ebx + 64]",
        "call dword ptr [ebp + 12]",
        "pop edi",
        "pop esi",
        "pop ebx",
        "pop ebp",
        "ret",
    );
}

#[unsafe(naked)]
unsafe extern "C" fn release_native(_target: usize, _compiled: usize) {
    core::arch::naked_asm!("mov eax, [esp + 8]", "jmp dword ptr [esp + 4]");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(kind: u32, count: u32, body: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        for word in [kind, count, 12 + body.len() as u32] {
            result.extend_from_slice(&word.to_le_bytes());
        }
        result.extend_from_slice(body);
        result
    }

    fn clip(frames: &[f32]) -> Vec<u8> {
        let mut keys = Vec::new();
        for &frame in frames {
            keys.extend_from_slice(&1.0_f32.to_le_bytes());
            keys.extend_from_slice(&frame.to_le_bytes());
        }
        let channel = block(0x8021_0001, frames.len() as u32, &keys);
        let track = block(0x38, 1, &channel);
        let mut body = vec![0; 8];
        body.extend(track);
        block(1, 1, &body)
    }

    fn skeleton(count: usize, tag: u16) -> Vec<[u8; NODE_SIZE]> {
        let mut nodes = vec![[0; NODE_SIZE]; count];
        let base = nodes.as_mut_ptr() as usize;
        for index in 0..count {
            unsafe {
                put(base + NODE_SIZE * index + NODE_COUNT, count as u16);
                put(base + NODE_SIZE * index + NODE_TAG, tag);
                if index + 1 < count {
                    put(
                        base + NODE_SIZE * index + NODE_CHILD,
                        base + NODE_SIZE * (index + 1),
                    );
                }
            }
        }
        nodes
    }

    #[test]
    fn nonzero_tag_and_actual_traversal_determine_binding() {
        let nodes = skeleton(3, 42);
        let base = nodes.as_ptr() as usize;
        let plan = unsafe { BindingPlan::read(&[base], (base, 3), 3) }.unwrap();
        assert_eq!(unsafe { get::<u16>(base + NODE_TAG) }, 42);
        assert_eq!(
            plan.targets
                .iter()
                .map(|target| target.address)
                .collect::<Vec<_>>(),
            [base, base + NODE_SIZE, base + 2 * NODE_SIZE]
        );
    }

    #[test]
    fn multiple_roots_and_mixed_tags_bind_only_the_native_consumed_group() {
        let mut nodes = skeleton(8, 42);
        let base = nodes.as_mut_ptr() as usize;
        // The tag-42 node 2 is beneath a tag-7 barrier. Native 10008CC0
        // skips it when binding tag 42, but continues to sibling node 3.
        for (index, tag, child, sibling) in [
            (0, 42, Some(1), None),
            (1, 7, Some(2), Some(3)),
            (2, 42, None, None),
            (3, 42, Some(4), Some(5)),
            (4, 42, None, None),
            (5, 7, None, None),
            (6, 99, Some(7), None),
            (7, 99, None, None),
        ] {
            let node = base + index * NODE_SIZE;
            unsafe {
                put(node + NODE_COUNT, if index < 6 { 6_u16 } else { 2_u16 });
                put(node + NODE_TAG, tag as u16);
                put(node + NODE_CHILD, child.map_or(0, |i| base + i * NODE_SIZE));
                put(
                    node + NODE_SIBLING,
                    sibling.map_or(0, |i| base + i * NODE_SIZE),
                );
                put(node + NODE_MOTION, 0x1234_0000usize + index * 16);
                put(node + NODE_TRACK_OFFSET, 500 + index as u32);
            }
        }
        let roots = [base, base + 6 * NODE_SIZE];
        let original = nodes.clone();
        let binding = unsafe { BindingPlan::read(&roots, (base, 8), 3) }.unwrap();
        assert_eq!(
            binding
                .targets
                .iter()
                .map(|target| (target.address - base) / NODE_SIZE)
                .collect::<Vec<_>>(),
            [0, 3, 4]
        );
        let offsets = [96_u32, 160, 224];
        assert!(unsafe { binding.bind(0x2345_0000, &offsets[..2]) }.is_err());
        assert_eq!(nodes, original);
        unsafe { binding.bind(0x2345_0000, &offsets) }.unwrap();
        let mut expected = original.clone();
        for (&index, offset) in [0, 3, 4].iter().zip(offsets) {
            expected[index][NODE_MOTION..NODE_MOTION + 4]
                .copy_from_slice(&0x2345_0000_u32.to_le_bytes());
            expected[index][NODE_TRACK_OFFSET..NODE_TRACK_OFFSET + 4]
                .copy_from_slice(&offset.to_le_bytes());
        }
        assert_eq!(
            nodes, expected,
            "other tags and the second root are untouched"
        );
        unsafe { binding.restore() };
        assert_eq!(nodes, original);

        // Both tag 7 and the independent tag-99 root consume two tracks.
        // Neither can be chosen merely because it was encountered first.
        let error = unsafe { BindingPlan::read(&roots, (base, 8), 2) }
            .err()
            .unwrap();
        assert!(error.contains("根 0 / tag 7"));
        assert!(error.contains("根 1 / tag 99"));
        // Node 2 does not become an extra tag-42 entry: 10008D30 already
        // found that tag at node 0, whose consumer skips node 2's parent.
        assert!(unsafe { BindingPlan::read(&roots, (base, 8), 1) }.is_err());
        assert_eq!(nodes, original);

        // Validate even the unselected root before changing any binding.
        unsafe { put(base + 7 * NODE_SIZE + NODE_CHILD, base + 6 * NODE_SIZE) };
        let malformed = nodes.clone();
        assert!(unsafe { BindingPlan::read(&roots, (base, 8), 3) }.is_err());
        assert_eq!(nodes, malformed);
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn actual_em001_motion_groups_keep_other_tags_and_the_tail_root_unbound() {
        use mhf_resource::{
            container::{SimpleArchive, open_layers},
            fskl::Fskl,
            motion::MotionArchive,
        };
        let game_root =
            std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        for name in ["dat/emmodel/em001.pac", "dat/emmodel-hd/em001-hd.pac"] {
            let bytes = std::fs::read(game_root.join(name)).unwrap();
            let opened = open_layers(&bytes, usize::MAX, usize::MAX).unwrap();
            let package = SimpleArchive::parse(opened.payload(), usize::MAX).unwrap();
            let model = open_layers(package.payload(0).unwrap(), usize::MAX, usize::MAX).unwrap();
            let members = SimpleArchive::parse(model.payload(), usize::MAX).unwrap();
            let skeleton_source =
                open_layers(members.payload(1).unwrap(), usize::MAX, usize::MAX).unwrap();
            let source = Fskl::parse(skeleton_source.payload()).unwrap();
            assert_eq!(source.nodes.len(), 48);
            assert_eq!(source.root_indices(), [0, 45]);

            let mut nodes = vec![[0_u8; NODE_SIZE]; source.nodes.len()];
            let base = nodes.as_mut_ptr() as usize;
            for (index, bone) in source.bones().enumerate() {
                let node = base + index * NODE_SIZE;
                let address = |index: i32| {
                    if index < 0 {
                        0
                    } else {
                        base + index as usize * NODE_SIZE
                    }
                };
                unsafe {
                    // 100022A0 copies motion_tag into the compact node's
                    // WORD +2; 10009DD0 places it at runtime node +198.
                    put(node + NODE_TAG, bone.motion_tag as u16);
                    put(node + NODE_COUNT, if index < 45 { 45_u16 } else { 3_u16 });
                    put(node + NODE_CHILD, address(bone.first_child_index));
                    put(node + NODE_SIBLING, address(bone.next_sibling_index));
                }
            }
            let roots = source
                .root_indices()
                .iter()
                .map(|&root| base + root as usize * NODE_SIZE)
                .collect::<Vec<_>>();
            let motion_data =
                open_layers(package.payload(2).unwrap(), usize::MAX, usize::MAX).unwrap();
            let data = motion_data.payload();
            let group_count = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize / 8;
            let motions = MotionArchive::parse(data, group_count).unwrap();
            let mut checked = BTreeMap::new();
            for (group, entry) in motions.groups.iter().enumerate() {
                for (slot, offset) in entry.motion_offsets.iter().enumerate() {
                    if offset.is_none() {
                        continue;
                    }
                    let motion = motions.motion(group, slot).unwrap().unwrap();
                    // The 45-node body is split into three actual MOT groups;
                    // it does not contain a single 45-track body clip.
                    let expected = match motion.tracks.len() {
                        29 => 0..29,
                        11 => 29..40,
                        5 => 40..45,
                        count => panic!("{name}: unexpected {count}-track motion"),
                    };
                    MotionPlan::read(&motion).unwrap();
                    let original = nodes.clone();
                    let binding =
                        unsafe { BindingPlan::read(&roots, (base, 48), motion.tracks.len()) }
                            .unwrap();
                    let mut selected = binding
                        .targets
                        .iter()
                        .map(|target| (target.address - base) / NODE_SIZE)
                        .collect::<Vec<_>>();
                    selected.sort_unstable();
                    assert_eq!(selected, expected.clone().collect::<Vec<_>>());
                    let offsets = (0..motion.tracks.len())
                        .map(|index| 256 + index as u32 * 8)
                        .collect::<Vec<_>>();
                    unsafe { binding.bind(0x2345_0000, &offsets) }.unwrap();
                    for index in 0..nodes.len() {
                        if !expected.contains(&index) {
                            assert_eq!(
                                nodes[index], original[index],
                                "{name}: unrelated node {index}"
                            );
                        }
                    }
                    unsafe { binding.restore() };
                    assert_eq!(nodes, original, "{name}: group {group} slot {slot}");
                    *checked.entry(motion.tracks.len()).or_insert(0) += 1;
                }
            }
            assert_eq!(checked, BTreeMap::from([(5, 75), (11, 78), (29, 73)]));
            eprintln!(
                "{name}: 226 motions match their exact 29/11/5-node tag group; other tags and the 3-node tail stay intact"
            );
        }
    }

    #[test]
    fn wide_and_deep_skeletons_over_64_bind_all_tracks_and_restore_every_byte() {
        for deep in [false, true] {
            let count = 257;
            let mut nodes = skeleton(count, 42);
            let base = nodes.as_mut_ptr() as usize;
            for index in 0..count {
                unsafe {
                    put(
                        base + index * NODE_SIZE + NODE_MOTION,
                        0x1234_0000usize + index * 16,
                    );
                    put(
                        base + index * NODE_SIZE + NODE_TRACK_OFFSET,
                        500 + index as u32,
                    );
                    if !deep {
                        // One root with 256 siblings, in reverse physical order.
                        put(
                            base + index * NODE_SIZE + NODE_CHILD,
                            if index == 0 {
                                base + (count - 1) * NODE_SIZE
                            } else {
                                0
                            },
                        );
                        put(
                            base + index * NODE_SIZE + NODE_SIBLING,
                            if index > 1 {
                                base + (index - 1) * NODE_SIZE
                            } else {
                                0
                            },
                        );
                    }
                }
            }
            let original = nodes.clone();
            let binding = unsafe { BindingPlan::read(&[base], (base, count), count) }.unwrap();
            let order: Vec<_> = binding
                .targets
                .iter()
                .map(|target| (target.address - base) / NODE_SIZE)
                .collect();
            let expected_order: Vec<_> = if deep {
                (0..count).collect()
            } else {
                std::iter::once(0).chain((1..count).rev()).collect()
            };
            assert_eq!(order, expected_order);
            let offsets: Vec<_> = (0..count)
                .map(|index| 32 + count as u32 * 4 + index as u32 * 8)
                .collect();

            // A partial offset table must fail before any binding is overwritten.
            assert!(unsafe { binding.bind(0x2345_0000, &offsets[..count - 1]) }.is_err());
            assert!(unsafe { binding.bind(0, &offsets) }.is_err());
            assert_eq!(nodes, original);

            unsafe { binding.bind(0x2345_0000, &offsets) }.unwrap();
            let mut expected = original.clone();
            for (&index, &offset) in order.iter().zip(&offsets) {
                expected[index][NODE_MOTION..NODE_MOTION + 4]
                    .copy_from_slice(&0x2345_0000_u32.to_le_bytes());
                expected[index][NODE_TRACK_OFFSET..NODE_TRACK_OFFSET + 4]
                    .copy_from_slice(&offset.to_le_bytes());
            }
            assert_eq!(
                nodes, expected,
                "deep={deep}: only the two binding fields may change"
            );
            unsafe { binding.restore() };
            assert_eq!(
                nodes, original,
                "deep={deep}: rollback restores links, tags and offsets"
            );
        }
    }

    #[test]
    fn deep_hierarchy_errors_still_fail_without_changing_any_node() {
        let count = 257;
        let mut nodes = skeleton(count, 42);
        let base = nodes.as_mut_ptr() as usize;
        let last = base + (count - 1) * NODE_SIZE;
        for invalid in 0..3 {
            let original = nodes.clone();
            unsafe {
                match invalid {
                    0 => put(last + NODE_TAG, 43_u16),
                    1 => put(last + NODE_CHILD, base),
                    _ => put(last + NODE_SIBLING, base + count * NODE_SIZE),
                }
            }
            let malformed = nodes.clone();
            assert!(unsafe { BindingPlan::read(&[base], (base, count), count) }.is_err());
            assert_eq!(nodes, malformed);
            nodes.copy_from_slice(&original);
        }
    }

    // Test-only native-ABI receivers forward to independent C-ABI reference
    // helpers. No client DLL is needed to verify EDI/ESI and stack arguments.
    #[unsafe(naked)]
    unsafe extern "C" fn curve_test_bridge(
        _track: usize,
        _values: usize,
        _frame: f32,
        _cache: usize,
    ) -> i32 {
        core::arch::naked_asm!(
            "push ebp", "mov ebp, esp",
            "push dword ptr [ebp + 20]", "push dword ptr [ebp + 16]",
            "push dword ptr [ebp + 12]", "push dword ptr [ebp + 8]", "push edi",
            "call {reference}", "add esp, 20", "pop ebp", "ret",
            reference = sym curve_reference,
        );
    }

    unsafe extern "C" fn curve_reference(
        compiled: usize,
        track: usize,
        values: usize,
        frame: f32,
        cache: usize,
    ) -> i32 {
        unsafe {
            let value = get::<f32>(compiled) + get::<f32>(track) + frame;
            put(values + 24, value);
            put(values + 12, frame * 2.0);
            put(cache, 7_u16);
        }
        1
    }

    #[unsafe(naked)]
    unsafe extern "C" fn matrix_test_bridge() {
        core::arch::naked_asm!(
            "push edi", "push esi", "call {reference}", "add esp, 8", "ret",
            reference = sym matrix_reference,
        );
    }

    unsafe extern "C" fn matrix_reference(values: usize, matrix: usize) {
        let values: [f32; 10] = unsafe { get(values) };
        let mut result = [0.0; 16];
        result[..10].copy_from_slice(&values);
        result[12..15].copy_from_slice(&values[6..9]);
        result[15] = 1.0;
        unsafe { put(matrix, result) };
    }

    #[test]
    fn sampling_bridge_matches_per_node_reference_without_following_links() {
        let mut compiled = [0_u8; 64];
        compiled[..4].copy_from_slice(&13.0_f32.to_le_bytes());
        compiled[32..36].copy_from_slice(&17.0_f32.to_le_bytes());
        let compiled_pointer = compiled.as_ptr() as usize;
        for bound in [false, true] {
            for frame in [-1.0, 0.0, 3.75] {
                let mut actual = [0xa5_u8; NODE_SIZE];
                let node = actual.as_mut_ptr() as usize;
                unsafe {
                    put(
                        node + 220,
                        [1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
                    );
                    put(node + NODE_MOTION, if bound { compiled_pointer } else { 0 });
                    put(node + NODE_TRACK_OFFSET, 32_u32);
                    // Deliberately invalid hierarchy links must remain untouched;
                    // the per-node sampler must not read or recurse through them.
                    put(node + NODE_CHILD, 0xdead_0000usize);
                    put(node + NODE_SIBLING, 0xbeef_0000usize);
                }
                let mut expected = actual;
                let expected_node = expected.as_mut_ptr() as usize;
                unsafe {
                    let bind_values: [u8; 40] = get(expected_node + 220);
                    put(expected_node + 260, bind_values);
                    if bound {
                        curve_reference(
                            compiled_pointer,
                            compiled_pointer + 32,
                            expected_node + 260,
                            frame,
                            expected_node + 348,
                        );
                    }
                    matrix_reference(expected_node + 260, expected_node + 64);
                    sample_with(
                        curve_test_bridge as *const () as usize,
                        matrix_test_bridge as *const () as usize,
                        node,
                        frame,
                    );
                }
                assert_eq!(actual, expected, "bound={bound}, frame={frame}");
            }
        }
    }

    #[test]
    fn mixed_tags_bad_counts_cycles_and_outside_links_do_not_change_nodes() {
        let mut nodes = skeleton(3, 42);
        let base = nodes.as_mut_ptr() as usize;
        for (offset, value) in [
            (NODE_SIZE + NODE_TAG, 43usize),
            (NODE_CHILD, base),
            (NODE_CHILD, base + 3 * NODE_SIZE),
        ] {
            let original = nodes.clone();
            unsafe {
                if offset == NODE_SIZE + NODE_TAG {
                    put(base + offset, value as u16);
                } else {
                    put(base + offset, value);
                }
            }
            let malformed = nodes.clone();
            assert!(unsafe { BindingPlan::read(&[base], (base, 3), 3) }.is_err());
            assert_eq!(nodes, malformed);
            nodes.copy_from_slice(&original);
        }
        assert!(unsafe { BindingPlan::read(&[base], (base, 3), 2) }.is_err());
        assert!(unsafe { BindingPlan::read(&[base, base + NODE_SIZE], (base, 3), 3) }.is_err());
    }

    #[test]
    fn mot_plan_checks_finite_keys_and_native_allocation_size() {
        let source = clip(&[-1.0, 10.0]);
        let motion = Motion::parse(&source).unwrap();
        let plan = MotionPlan::read(&motion).unwrap();
        assert_eq!(plan.frames, 10.0);
        assert_eq!(plan.compiled_bytes, 32 + 4 + 8 + 8 + 16);
        // All six native searchers return one key for an exact frame match;
        // they return an interpolation pair only for current < frame < next.
        for frames in [&[1.0, 1.0][..], &[0.0, 1.0, 1.0, 2.0]] {
            let source = clip(frames);
            let motion = Motion::parse(&source).unwrap();
            MotionPlan::read(&motion).unwrap();
            assert_eq!(motion.as_bytes(), source);
        }
        for frames in [&[0.0, f32::NAN][..], &[2.0, 1.0]] {
            let source = clip(frames);
            assert!(MotionPlan::read(&Motion::parse(&source).unwrap()).is_err());
        }
    }

    #[test]
    fn compiled_track_and_key_ranges_are_checked_before_binding() {
        let source = clip(&[0.0, 10.0]);
        let motion = Motion::parse(&source).unwrap();
        let plan = MotionPlan::read(&motion).unwrap();
        let mut compiled = vec![0; plan.compiled_bytes];
        compiled[2..4].copy_from_slice(&1u16.to_le_bytes());
        compiled[8..12].copy_from_slice(&10.0f32.to_le_bytes());
        compiled[16..20].copy_from_slice(&32u32.to_le_bytes());
        compiled[24..28].copy_from_slice(&(plan.compiled_bytes as u32).to_le_bytes());
        compiled[32..36].copy_from_slice(&36u32.to_le_bytes());
        compiled[36..38].copy_from_slice(&0x1000u16.to_le_bytes());
        compiled[38..40].copy_from_slice(&1u16.to_le_bytes());
        compiled[40..44].copy_from_slice(&44u32.to_le_bytes());
        compiled[44] = 0x21;
        compiled[46..48].copy_from_slice(&2u16.to_le_bytes());
        compiled[48..52].copy_from_slice(&52u32.to_le_bytes());
        assert_eq!(compiled_offsets(&compiled, &motion, &plan).unwrap(), [36]);
        compiled[48..52].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(compiled_offsets(&compiled, &motion, &plan).is_err());
    }
}
