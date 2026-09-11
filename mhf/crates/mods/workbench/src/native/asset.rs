//! Native resource preview ownership, using the same dynamic pools as 113DA8C0.
//!
//! Loading/releasing runs on the native task thread. Drawing runs in the native
//! world-render phase after task work has finished. Never hold a render-thread
//! mutex while waiting on 108F88E0/108F8EB0: their texture/mesh work dispatches to
//! the game's render worker. The caller retains this asset until queued drawing
//! is complete and explicitly releases it before scene teardown.

use super::{
    Client,
    skeleton::Skeleton,
    textures::{self, NativeTextures},
};
use crate::preview::{AssetBundle, Bone, LoadedMesh};
use mhf_resource::{
    fmod::{Component, FaceGroup, Fmod, MaterialEntry, ObjectEntry, Section, TextureEntry},
    fskl::{Fskl, NodeEntry},
    txb::Image,
};
use std::{
    mem::{size_of, transmute},
    ptr,
    sync::Arc,
};
use windows::Win32::System::Threading::{
    CRITICAL_SECTION, EnterCriticalSection, LeaveCriticalSection,
};

const RESOURCE_POOL: usize = 0x1ed5_28d0;
const RESOURCE_FLAGS: usize = 0x1dbf_eeb8;
const RESOURCE_LOCK: usize = 0x1e73_ad40;

/// `108F8EB0` and `108F88E0` share this exact 128-byte runtime object. Fields
/// here are process pointers/indices, deliberately separate from FMOD types.
#[repr(C)]
#[derive(Clone, Copy)]
struct Resource {
    active: u16,
    unknown_02: u16,
    model_source: usize,
    texture_count: i16,
    texture_start: i16,
    material_count: i16,
    material_start: i16,
    materials: usize,
    unknown_14: [u8; 12],
    node_count: i16,
    node_start: i16,
    nodes: usize,
    alternate_nodes: usize,
    mesh_count: i16,
    mesh_start: i16,
    meshes: usize,
    root_count: u32,
    unknown_38: [u8; 12],
    root_nodes: [usize; 4],
    root_counts_or_alternates: [u32; 4],
    root_storage: [usize; 4],
    skeleton_mode: u8,
    shared: u8,
    unknown_76: [u8; 10],
}

/// Each object points into the resource's global material table. Native draw
/// binds these local slots before issuing the registered model handle.
#[repr(C)]
struct Mesh {
    handle: i32,
    material_count: u32,
    materials: [u32; 32],
    render_options: u32,
}

const _: () = assert!(size_of::<Resource>() == 128);
const _: () = assert!(size_of::<Mesh>() == 140);

#[derive(Debug)]
struct Requirements {
    mesh_vertices: Vec<usize>,
    materials: usize,
    textures: usize,
    /// Per-mesh shader slot -> node index. Preflight uses FSKL source indices;
    /// load remaps them once to the compiled skeleton's actual node order.
    bone_maps: Vec<Vec<(u32, usize)>>,
    node_ids: Vec<u16>,
    center: [f32; 3],
    radius: f32,
}

/// The native loader is not bounds checked. Resource inspection accepts more
/// layouts than this verified native rendering path; reject those explicitly
/// before any pool or texture state changes, preserving the original document.
fn validate_files(
    model: &[u8],
    skeleton: Option<&[u8]>,
    textures: &[&[u8]],
) -> Result<Requirements, String> {
    let model = Fmod::parse(model).map_err(|e| e.to_string())?;
    let textures = textures::images(textures)?;
    if textures.is_empty() {
        return Err("原生材质需要默认贴图槽 0，请追加 PNG、DDS 或 TXB 贴图".into());
    }
    let table = model
        .sections
        .iter()
        .find_map(|section| match section {
            Section::Materials(v) => Some(v),
            _ => None,
        })
        .ok_or("模型缺少原生材质表")?;
    let images = model
        .sections
        .iter()
        .find_map(|section| match section {
            Section::Textures(v) => Some(v),
            _ => None,
        })
        .ok_or("模型缺少原生贴图引用表")?;
    let meshes = model
        .sections
        .iter()
        .find_map(|section| match section {
            Section::Meshes(v) => Some(v),
            _ => None,
        })
        .ok_or("模型缺少 MAIN 块")?;
    if meshes.entries.is_empty() || table.records.is_empty() {
        return Err("模型没有可预览的网格或材质".into());
    }
    let mut node_ids = Vec::new();
    if let Some(skeleton) = skeleton {
        let skeleton = Fskl::parse(skeleton).map_err(|e| e.to_string())?;
        skeleton.validate_hierarchy().map_err(|e| e.to_string())?;
        if skeleton.nodes.is_empty() || skeleton.nodes.len() > i16::MAX as usize {
            return Err("骨架节点数量超出原生资源头的有符号 WORD 范围".into());
        }
        if skeleton.root_tables.len() != 1 || skeleton.blocks.len() != skeleton.nodes.len() + 1 {
            return Err("当前预览尚未支持含额外元数据的骨架编译布局".into());
        }
        if skeleton.root_indices().is_empty() || skeleton.root_indices().len() > 4 {
            return Err("原生资源头需要有效骨架根，最多容纳 4 组骨架".into());
        }
        // Native 100022A0 copies a contiguous node range for each root traversal.
        let mut visited = vec![false; skeleton.nodes.len()];
        for &root in skeleton.root_indices() {
            let mut stack = vec![root as usize];
            let mut subtree = Vec::new();
            while let Some(index) = stack.pop() {
                if visited[index] {
                    return Err("骨架节点顺序不符合原生连续子树布局".into());
                }
                visited[index] = true;
                subtree.push(index);
                let NodeEntry::Bone(node) = &skeleton.nodes[index] else {
                    return Err("未知原生骨骼布局".into());
                };
                if node.next_sibling_index >= 0 {
                    stack.push(node.next_sibling_index as usize);
                }
                if node.first_child_index >= 0 {
                    stack.push(node.first_child_index as usize);
                }
            }
            // The compiler copies a contiguous index range; it does not require
            // child/sibling traversal to encounter that range in ascending order.
            let end = root as usize + subtree.len();
            if subtree
                .iter()
                .any(|&index| index < root as usize || index >= end)
            {
                return Err("骨架根所引用的节点不构成原生要求的连续索引范围".into());
            }
        }
        if visited.iter().any(|v| !v) {
            return Err("存在不属于任何根骨架的节点".into());
        }
        for bone in skeleton.bones() {
            let id = u16::try_from(bone.node_id).map_err(|_| "骨架节点 ID 超出原生 WORD 范围")?;
            if node_ids.contains(&id) {
                return Err("骨架节点 ID 重复，无法唯一绑定网格矩阵".into());
            }
            if bone.transform.scale[..3]
                .iter()
                .any(|v| !v.is_finite() || *v == 0.0)
                || bone.transform.rotation[..3]
                    .iter()
                    .chain(&bone.transform.translation[..3])
                    .any(|v| !v.is_finite())
            {
                return Err("骨骼变换包含无法用于原生矩阵运算的值".into());
            }
            node_ids.push(id);
        }
    }
    if textures.len() > textures::TEXTURE_CAPACITY {
        return Err(format!(
            "此资源需要 {} 张贴图，超过原生 4095 个可分配贴图句柄",
            textures.len()
        ));
    }
    for texture in &textures {
        match texture {
            Image::Png(png) => png.validate().map_err(|e| e.to_string())?,
            Image::Dds(dds) => {
                // Every stored surface consumes at least one pixel-data byte.
                dds.surfaces(dds.pixel_data().len())
                    .map_err(|e| e.to_string())?;
            }
            Image::Empty => return Err("此原生贴图分配流程不支持空槽位".into()),
            Image::Unknown(_) => return Err("此资源含尚未确认可由原生加载的贴图格式".into()),
        }
    }
    for texture in &images.records {
        let TextureEntry::Texture(texture) = texture else {
            return Err("未知原生贴图引用记录".into());
        };
        if texture.image_id as usize >= textures.len() {
            return Err(format!(
                "贴图 image ID {} 尚未绑定，当前提供 {} 张图片，请追加所需贴图",
                texture.image_id,
                textures.len()
            ));
        }
    }
    for material in &table.records {
        let MaterialEntry::Material(material) = material else {
            return Err("未知原生材质记录".into());
        };
        // 100027B0 consumes only the first four references, even when the
        // material carries more. Extra source channels remain untouched.
        if material
            .texture_indices
            .iter()
            .take(4)
            .any(|&i| i as usize >= images.records.len())
        {
            return Err("材质贴图引用超出贴图表范围".into());
        }
    }
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    let mut bone_maps = Vec::new();
    let mut mesh_vertices = Vec::with_capacity(meshes.entries.len());
    for (object_index, object) in meshes.entries.iter().enumerate() {
        let ObjectEntry::Object(object) = object else {
            return Err("MAIN 中含未知对象，不能按原生序号加载".into());
        };
        object.validate_geometry().map_err(|e| e.to_string())?;
        let positions = object.positions().ok_or("网格缺少位置数组")?;
        if positions.values.is_empty() || object.normals().is_none() || object.colors().is_none() {
            return Err("原生网格加载要求完整的位置、法线和颜色数组".into());
        }
        mesh_vertices.push(positions.values.len());
        for position in &positions.values {
            if position.iter().any(|value| !value.is_finite()) {
                return Err("网格位置包含非有限值".into());
            }
            for axis in 0..3 {
                minimum[axis] = minimum[axis].min(position[axis]);
                maximum[axis] = maximum[axis].max(position[axis]);
            }
        }
        if positions.values.len() > i32::MAX as usize / 72 {
            return Err("原生临时顶点缓冲区长度溢出".into());
        }
        let list = object
            .components
            .iter()
            .find_map(|c| {
                if let Component::MaterialList(v) = c {
                    Some(v)
                } else {
                    None
                }
            })
            .ok_or("网格缺少材质列表")?;
        let map = object
            .components
            .iter()
            .find_map(|c| {
                if let Component::MaterialMap(v) = c {
                    Some(v)
                } else {
                    None
                }
            })
            .ok_or("网格缺少逐条带材质映射")?;
        if list.values.is_empty()
            || list.values.len() > 32
            || list
                .values
                .iter()
                .any(|&i| i as usize >= table.records.len())
        {
            return Err("网格材质超出原生 32 个局部槽位或全局材质表".into());
        }
        let faces = object.faces().ok_or("网格缺少面数据")?;
        if faces
            .groups
            .iter()
            .any(|g| matches!(g, FaceGroup::Unknown(_)))
        {
            return Err("原生预览尚不支持此面组类型".into());
        }
        let strip_count = faces.strips().count();
        if strip_count == 0
            || faces.strips().any(|(_, strip)| strip.indices.len() < 3)
            || map.values.len() < strip_count
            || map.values.iter().any(|&i| i as usize >= list.values.len())
        {
            return Err("面条带或对应材质映射不适合原生绘制".into());
        }
        // 10002560 reads 18 words even when a block's own payload is short.
        for component in &object.components {
            let block = component.block();
            if block.header.kind == 0xf0000 && block.payload().len() < 72 {
                return Err("原生渲染配置块不足 72 字节".into());
            }
        }
        let node_map = object.components.iter().find_map(|c| {
            if let Component::BoneMap(v) = c {
                Some(v)
            } else {
                None
            }
        });
        let mut palette = Vec::new();
        if let Some(weights) = object.weights() {
            if let Some(map) = node_map {
                if map.values.is_empty() || map.values.len() > 32 {
                    return Err("单个网格的蒙皮矩阵超过原生 32 个上传槽位".into());
                }
                for (slot, &id) in map.values.iter().enumerate() {
                    let node = u16::try_from(id)
                        .ok()
                        .and_then(|id| node_ids.iter().position(|&node| node == id))
                        .ok_or("网格蒙皮矩阵引用了绑定骨架中不存在的节点")?;
                    palette.push((slot as u32, node));
                }
            } else {
                // Without an explicit palette, vertex blend indices are the
                // native shader slots themselves, not a compacted list of IDs.
                palette.extend(
                    node_ids
                        .iter()
                        .enumerate()
                        .filter_map(|(node, &id)| (id < 32).then_some((u32::from(id), node))),
                );
            }
            if object.uvs().is_none() {
                return Err("原生蒙皮顶点路径要求 UV 数组".into());
            }
            let has_zero_slot = palette.iter().any(|&(slot, _)| slot == 0);
            for (vertex_index, vertex) in weights.vertices.iter().enumerate() {
                if vertex.influences.len() > 4 {
                    return Err(format!(
                        "网格 {object_index} 顶点 {vertex_index} 的 {} 个影响超过原生转换器的 4 项缓冲区",
                        vertex.influences.len()
                    ));
                }
                // 10003790 clears four index/weight words, skips the input
                // loop for count=0, then adds 255-sum to weight[0]. This is an
                // implicit full influence from shader slot 0, not bad data.
                // Keep the original zero-count record and let native pack it.
                if vertex.influences.is_empty() && !has_zero_slot {
                    return Err(format!(
                        "网格 {object_index} 顶点 {vertex_index} 的零影响记录需要可绑定的原生骨骼槽位 0"
                    ));
                }
                for (influence_index, influence) in vertex.influences.iter().enumerate() {
                    let valid_index = node_map.map_or_else(
                        || {
                            influence.bone_index < 32
                                && node_ids.contains(&(influence.bone_index as u16))
                        },
                        |m| (influence.bone_index as usize) < m.values.len(),
                    );
                    if !valid_index {
                        return Err(format!(
                            "网格 {object_index} 顶点 {vertex_index} 影响 {influence_index} 的骨骼槽位 {} 超出绑定范围",
                            influence.bone_index
                        ));
                    }
                    // 10003790 truncates the scaled float and packs its low
                    // BYTE. It imposes no percentage range or normalization.
                    if !influence.weight.is_finite() {
                        return Err(format!(
                            "网格 {object_index} 顶点 {vertex_index} 影响 {influence_index} 的权重 {:?}（{:#010X}）不是有限值",
                            influence.weight,
                            influence.weight.to_bits()
                        ));
                    }
                }
            }
        }
        bone_maps.push(palette);
    }
    if table.records.len() > i16::MAX as usize || meshes.entries.len() > i16::MAX as usize {
        return Err("资源数量超出原生有符号 WORD 计数".into());
    }
    Ok(Requirements {
        mesh_vertices,
        materials: table.records.len(),
        textures: textures.len(),
        bone_maps,
        node_ids,
        center: std::array::from_fn(|axis| minimum[axis] * 0.5 + maximum[axis] * 0.5),
        radius: maximum
            .into_iter()
            .zip(minimum)
            .map(|(max, min)| (max * 0.5 - min * 0.5).powi(2))
            .sum::<f32>()
            .sqrt(),
    })
}

/// All addresses are preferred VAs, relocated through Client. Prefixes stop
/// before absolute operands, so ASLR does not invalidate the signature check.
const SIGNATURES: &[(usize, &[u8])] = &[
    (
        0x108f88e0,
        &[
            0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0xb8, 0x2c, 0x12, 0x00, 0x00,
        ],
    ),
    (
        0x108f8eb0,
        &[
            0x55, 0x8b, 0xec, 0x8b, 0x45, 0x08, 0x56, 0x8b, 0xf0, 0xc1, 0xe6, 0x07,
        ],
    ),
    (
        0x108f80a0,
        &[
            0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x53, 0x56, 0x57, 0x8b, 0x7d, 0x08,
        ],
    ),
    (
        0x108f7e90,
        &[
            0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x57, 0x8b, 0x7d, 0x08, 0x33, 0xc0,
        ],
    ),
    (
        0x108f7ff0,
        &[
            0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x57, 0x8b, 0x7d, 0x08, 0x33, 0xc0,
        ],
    ),
    (
        0x108f7f40,
        &[
            0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x53, 0x56, 0x57, 0x8b, 0x7d, 0x08,
        ],
    ),
    (
        0x10007f50,
        &[
            0x55, 0x8b, 0xec, 0x51, 0x85, 0xc0, 0x75, 0x05, 0x33, 0xc0, 0x59, 0x5d,
        ],
    ),
    (
        0x108f8440,
        &[0x55, 0x8b, 0xec, 0x51, 0x53, 0xf6, 0xc1, 0x01, 0x0f, 0x84],
    ),
    // 108F8520 resets material blend/filter overrides; skip its relocated
    // initial MOV operand and verify the following native guard sequence.
    (
        0x108f8526,
        &[0xf7, 0xd8, 0x1b, 0xc0, 0xf7, 0xd0, 0x85, 0x05],
    ),
];

pub(crate) fn preflight(bundle: &AssetBundle) -> Result<(), String> {
    let textures = bundle
        .textures
        .iter()
        .map(|source| source.bytes())
        .collect::<Result<Vec<_>, _>>()?;
    validate_files(
        bundle.model.bytes()?,
        bundle
            .skeleton
            .as_ref()
            .map(|source| source.bytes())
            .transpose()?,
        &textures,
    )
    .map(|_| ())
}

pub(crate) unsafe fn validate(client: Client) -> Result<(), String> {
    for &(address, prefix) in SIGNATURES {
        if unsafe { std::slice::from_raw_parts(client.address(address) as *const u8, prefix.len()) }
            != prefix
        {
            return Err(format!("不支持此客户端的资源预览接口：{address:#x}"));
        }
    }
    unsafe { textures::validate_interfaces(client) }
}

#[must_use = "release on the native task thread before dropping the source document"]
pub(crate) struct NativeAsset {
    bundle: AssetBundle,
    requirements: Requirements,
    pool: usize,
    slot: Option<i32>,
    textures: NativeTextures,
    skeleton: Option<Skeleton>,
    meshes: Arc<Vec<LoadedMesh>>,
}

impl NativeAsset {
    /// # Safety
    /// Native task thread only, after scene pools initialize, while all drawing
    /// is quiescent. The supported geometry Mod must already own model hooks.
    pub(crate) unsafe fn load(client: Client, bundle: AssetBundle) -> Result<Self, String> {
        let model = bundle.model.bytes()?;
        let skeleton = bundle
            .skeleton
            .as_ref()
            .map(|source| source.bytes())
            .transpose()?;
        let textures = bundle
            .textures
            .iter()
            .map(|source| source.bytes())
            .collect::<Result<Vec<_>, _>>()?;
        let requirements = validate_files(model, skeleton, &textures)?;
        let mut native_textures = NativeTextures::prepare(model, requirements.textures)?;
        let model_source = native_textures.model_source();
        let skeleton_source = skeleton.map_or(0, |bytes| bytes.as_ptr() as usize);
        let pool = unsafe { client.read::<usize>(RESOURCE_POOL) };
        if pool == 0
            || unsafe {
                client.read::<usize>(0x1ed528c4) == 0
                    || client.read::<usize>(0x1ed528c8) == 0
                    || client.read::<usize>(0x1ed528cc) == 0
            }
        {
            return Err("游戏资源池尚未初始化".into());
        }
        // The native constructor does not safely unwind an exhausted sub-pool.
        // Check capacity before it performs texture uploads and allocations.
        for (find, lock, count, label) in [
            (0x108f7e90, 0x1e73ad58, requirements.materials, "材质"),
            (
                0x108f7ff0,
                0x1e73ad10,
                requirements.mesh_vertices.len(),
                "网格",
            ),
            (0x108f7f40, 0x1e73ad70, requirements.node_ids.len(), "骨架"),
        ] {
            if count == 0 {
                continue;
            }
            let find: unsafe extern "C" fn(i32) -> i32 = unsafe { transmute(client.address(find)) };
            let critical = client.address(lock) as *mut CRITICAL_SECTION;
            unsafe {
                EnterCriticalSection(critical);
            }
            let slot = unsafe { find(count as i32) };
            unsafe {
                LeaveCriticalSection(critical);
            }
            if slot < 0 {
                return Err(format!("原生{label}资源池空间不足"));
            }
        }
        unsafe { native_textures.upload(client, &textures) }?;
        let find: unsafe extern "C" fn(i32) -> i32 =
            unsafe { transmute(client.address(0x108f80a0)) };
        let critical = client.address(RESOURCE_LOCK) as *mut CRITICAL_SECTION;
        unsafe {
            EnterCriticalSection(critical);
        }
        let slot = unsafe { find(1) };
        if slot >= 0 {
            unsafe {
                ptr::write(
                    (client.address(RESOURCE_FLAGS) + slot as usize) as *mut u8,
                    1,
                );
            }
        }
        unsafe {
            LeaveCriticalSection(critical);
        }
        if !(0..1280).contains(&slot) {
            unsafe { native_textures.release(client) }?;
            return Err("原生资源对象池已满".into());
        }
        let resource = (pool + slot as usize * size_of::<Resource>()) as *mut Resource;
        unsafe {
            ptr::write_bytes(resource.cast::<u8>(), 0, size_of::<Resource>());
        }
        let mut asset = Self {
            meshes: Arc::new(
                requirements
                    .mesh_vertices
                    .iter()
                    .enumerate()
                    .map(|(index, &vertices)| LoadedMesh {
                        index,
                        vertices,
                        visible: true,
                    })
                    .collect(),
            ),
            bundle,
            requirements,
            pool,
            slot: Some(slot),
            textures: native_textures,
            skeleton: None,
        };
        let constructed = unsafe {
            asset
                .textures
                .with_constructor_slot(client, |texture_start, empty_txb| {
                    create_resource(
                        client.address(0x108f88e0),
                        skeleton_source,
                        empty_txb,
                        resource as usize,
                        model_source,
                        texture_start,
                    );
                })
        };
        if let Err(error) = constructed {
            // No constructor ran, so the still-zero resource is not active.
            // Return only the object flag reserved by this load attempt.
            unsafe {
                EnterCriticalSection(critical);
                ptr::write(
                    (client.address(RESOURCE_FLAGS) + slot as usize) as *mut u8,
                    0,
                );
                LeaveCriticalSection(critical);
                asset.textures.release(client)?;
            }
            asset.slot = None;
            return Err(error);
        }
        let result = unsafe {
            asset
                .check_loaded(client)
                .and_then(|()| {
                    asset
                        .textures
                        .bind_materials((*resource).materials, asset.requirements.materials)?;
                    for index in 0..asset.requirements.mesh_vertices.len() {
                        let mesh =
                            &*(((*resource).meshes + index * size_of::<Mesh>()) as *const Mesh);
                        asset.textures.bind_mesh_materials(
                            client,
                            mesh.handle as u32,
                            &mesh.materials[..mesh.material_count as usize],
                        )?;
                    }
                    Ok(())
                })
                .and_then(|()| asset.prepare_skeleton(client))
        };
        if let Err(error) = result {
            unsafe {
                asset.release(client)?;
            }
            return Err(error);
        }
        Ok(asset)
    }

    unsafe fn prepare_skeleton(&mut self, client: Client) -> Result<(), String> {
        let resource = unsafe { self.owned_resource(client) }
            .copied()
            .ok_or("模型资源已释放")?;
        let skeleton = unsafe {
            Skeleton::prepare(
                &resource.root_nodes[..resource.root_count as usize],
                (resource.nodes, self.requirements.node_ids.len()),
            )
        }?;
        for palette in &mut self.requirements.bone_maps {
            for (_, node) in palette {
                let id = self.requirements.node_ids[*node];
                *node = skeleton
                    .node_index(id)
                    .ok_or("原生编译骨架丢失了网格引用的节点 ID")?;
            }
        }
        self.skeleton = Some(skeleton);
        Ok(())
    }

    pub(crate) unsafe fn animation_nodes(
        &self,
        client: Client,
    ) -> Result<(&[usize], (usize, usize)), String> {
        let resource = unsafe { self.owned_resource(client) }.ok_or("模型资源已释放")?;
        if resource.root_count == 0 || resource.nodes == 0 {
            return Err("模型未绑定骨架，无法加载动画".into());
        }
        Ok((
            &resource.root_nodes[..resource.root_count as usize],
            (resource.nodes, self.requirements.node_ids.len()),
        ))
    }

    pub(crate) fn framing(&self) -> ([f32; 3], f32) {
        (
            self.requirements.center,
            (self.requirements.radius * 2.5).clamp(1.0, 100_000.0),
        )
    }

    pub(crate) fn name(&self) -> &str {
        &self.bundle.name
    }

    pub(crate) fn meshes(&self) -> Arc<Vec<LoadedMesh>> {
        self.meshes.clone()
    }

    pub(crate) fn bone_bindings(&self) -> Arc<Vec<Option<usize>>> {
        self.skeleton
            .as_ref()
            .map_or_else(Arc::default, Skeleton::bindings)
    }

    pub(crate) fn set_bone_binding(
        &mut self,
        node: usize,
        source: Option<usize>,
    ) -> Result<(), String> {
        self.skeleton
            .as_mut()
            .ok_or("模型骨架尚未初始化")?
            .set_binding(node, source)
    }

    pub(crate) fn clear_bone_bindings(&mut self) -> Result<(), String> {
        self.skeleton
            .as_mut()
            .ok_or("模型骨架尚未初始化")?
            .clear_bindings();
        Ok(())
    }

    pub(crate) fn set_mesh_visible(&mut self, index: usize, visible: bool) -> Result<(), String> {
        let mesh = self.meshes.get(index).ok_or("子网格编号超出当前模型范围")?;
        if mesh.visible != visible {
            Arc::make_mut(&mut self.meshes)[index].visible = visible;
        }
        Ok(())
    }

    pub(crate) fn isolate_mesh(&mut self, index: usize) -> Result<(), String> {
        if index >= self.meshes.len() {
            return Err("子网格编号超出当前模型范围".into());
        }
        for mesh in Arc::make_mut(&mut self.meshes) {
            mesh.visible = mesh.index == index;
        }
        Ok(())
    }

    pub(crate) fn show_all_meshes(&mut self) {
        for mesh in Arc::make_mut(&mut self.meshes) {
            mesh.visible = true;
        }
    }

    pub(crate) unsafe fn bones(&self, client: Client) -> Vec<Bone> {
        let Some(resource) = (unsafe { self.owned_resource(client) }) else {
            return Vec::new();
        };
        let count = self.requirements.node_ids.len();
        let base = resource.nodes;
        (0..count)
            .filter_map(|index| {
                let node = base + 448 * index;
                let position = unsafe { ptr::read_unaligned((node + 48) as *const [f32; 3]) };
                let parent = unsafe { ptr::read_unaligned((node + 200) as *const usize) };
                let parent = if parent == 0 {
                    None
                } else {
                    let offset = parent.checked_sub(base)?;
                    if !offset.is_multiple_of(448) || offset / 448 >= count {
                        return None;
                    }
                    Some(offset / 448)
                };
                position
                    .iter()
                    .all(|value| value.is_finite())
                    .then_some(Bone {
                        index,
                        parent,
                        position,
                    })
            })
            .collect()
    }

    unsafe fn owned_resource(&self, client: Client) -> Option<&Resource> {
        let slot = self.slot?;
        if unsafe { client.read::<usize>(RESOURCE_POOL) } != self.pool {
            return None;
        }
        let resource =
            unsafe { &*((self.pool + slot as usize * size_of::<Resource>()) as *const Resource) };
        (resource.active != 0
            && resource.shared == 0
            && resource.model_source == self.textures.model_source())
        .then_some(resource)
    }

    unsafe fn check_loaded(&self, client: Client) -> Result<(), String> {
        let resource = unsafe { self.owned_resource(client) }.ok_or("原生预览资源创建失败")?;
        if resource.mesh_count as usize != self.requirements.mesh_vertices.len()
            || resource.material_count as usize != self.requirements.materials
            || resource.node_count as usize != self.requirements.node_ids.len()
            || resource.meshes == 0
            || resource.materials == 0
            || resource.root_count > 4
            || resource.texture_count != 0
            || (!self.requirements.node_ids.is_empty()
                && (resource.nodes == 0 || resource.root_count == 0 || resource.root_nodes[0] == 0))
        {
            return Err("原生资源创建返回不完整的网格、材质或骨架".into());
        }
        for index in 0..self.requirements.mesh_vertices.len() {
            let mesh = unsafe { &*((resource.meshes + index * size_of::<Mesh>()) as *const Mesh) };
            if mesh.handle <= 0 {
                return Err(format!("原生网格 {index} 上传失败"));
            }
        }
        unsafe { self.textures.validate(client, self.requirements.textures) }
    }

    /// # Safety
    /// Native world-render thread/phase only, with no concurrent load/release.
    /// `world` must be finite and stay valid until this call completes.
    pub(crate) unsafe fn draw(
        &mut self,
        client: Client,
        world: &[f32; 16],
        frame: f32,
    ) -> Result<(), String> {
        if world.iter().any(|v| !v.is_finite()) {
            return Err("预览世界矩阵无效".into());
        }
        let resource = unsafe { self.owned_resource(client) }
            .copied()
            .ok_or("预览资源已随场景释放，请重新载入")?;
        let skeleton = self.skeleton.as_mut().ok_or("预览骨架尚未初始化")?;
        let matrices = unsafe { skeleton.update(client, frame, world) }?;
        let parameter: unsafe extern "fastcall" fn(usize, u32) -> i32 =
            unsafe { transmute(client.address(0x1000c7d0)) };
        let render_options: unsafe extern "fastcall" fn(u32) =
            unsafe { transmute(client.address(0x108f8440)) };
        let reset_render_options: unsafe extern "C" fn() =
            unsafe { transmute(client.address(0x108f8520)) };
        // Match the native draw helpers (e.g. 11203230): options with bit 0
        // unset inherit the caller's standard alpha blending. Establish that
        // baseline before the first mesh, then restore it after every draw.
        // Otherwise an additive mesh in em001's second model tints the first
        // model with the framebuffer/background on the following frame.
        unsafe { reset_render_options() };
        for index in 0..self.requirements.mesh_vertices.len() {
            if !self.meshes[index].visible {
                continue;
            }
            let mesh = unsafe { &*((resource.meshes + index * size_of::<Mesh>()) as *const Mesh) };
            let map = &self.requirements.bone_maps[index];
            unsafe {
                if map.is_empty() {
                    parameter(world.as_ptr() as usize, 26);
                    parameter(world.as_ptr() as usize, 27);
                } else {
                    for &(slot, node) in map {
                        parameter(matrices[node].as_ptr() as usize, 26 + slot);
                    }
                }
                for local in 0..mesh.material_count as usize {
                    parameter(
                        resource.materials + mesh.materials[local] as usize * 140,
                        58 + local as u32,
                    );
                }
                render_options(mesh.render_options);
                let drawn = draw_model(client.address(0x10007f50), mesh.handle as u32);
                // Also restore when the native handle failed to draw. This
                // updates both the device and the engine's cached state.
                reset_render_options();
                if drawn == 0 {
                    return Err(format!("网格 {index} 已失去原生绘制句柄"));
                }
            }
        }
        Ok(())
    }

    /// # Safety
    /// Task thread only, after all queued world rendering using this asset has
    /// finished. Does not free a slot already cleared/reused by scene teardown.
    pub(crate) unsafe fn release(&mut self, client: Client) -> Result<(), String> {
        if self.slot.is_none() {
            return unsafe { self.textures.release(client) };
        }
        let owned = unsafe { self.owned_resource(client) }.is_some();
        let slot = self.slot.take().unwrap();
        if owned {
            let release: unsafe extern "C" fn(i32) -> i32 =
                unsafe { transmute(client.address(0x108f8eb0)) };
            unsafe {
                release(slot);
            }
        }
        self.skeleton = None;
        unsafe { self.textures.release(client) }
    }
}

/// ECX/EDX register arguments and seven caller-cleaned stack arguments. The
/// client's function returns with plain RET, despite IDA's fastcall label.
#[unsafe(naked)]
unsafe extern "C" fn create_resource(
    _target: usize,
    _skeleton: usize,
    _textures: usize,
    _resource: usize,
    _model: usize,
    _texture_start: i32,
) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp,esp",
        "mov ecx,[ebp+12]",
        "mov edx,[ebp+16]",
        "xor eax,eax",
        "test ecx,ecx",
        "setnz al",
        "push eax",
        "push 0x100",
        "push 0",
        "push dword ptr [ebp+28]",
        "push 0",
        "push dword ptr [ebp+24]",
        "push dword ptr [ebp+20]",
        "call dword ptr [ebp+8]",
        "add esp,28",
        "pop ebp",
        "ret",
    );
}

#[unsafe(naked)]
unsafe extern "C" fn draw_model(_target: usize, _handle: u32) -> i32 {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp,esp",
        "mov eax,[ebp+12]",
        "push 0",
        "call dword ptr [ebp+8]",
        "add esp,4",
        "pop ebp",
        "ret",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset_with_meshes(vertex_counts: &[usize]) -> NativeAsset {
        let document = Arc::new(crate::inspect::inspect("mesh fixture", Arc::from([])));
        let source = crate::preview::ResourceRef { document, node: 0 };
        NativeAsset {
            bundle: AssetBundle {
                model: source.clone(),
                skeleton: None,
                textures: vec![source],
                name: "mesh fixture".into(),
            },
            requirements: Requirements {
                mesh_vertices: vertex_counts.to_vec(),
                materials: 0,
                textures: 0,
                bone_maps: vec![Vec::new(); vertex_counts.len()],
                node_ids: Vec::new(),
                center: [0.0; 3],
                radius: 1.0,
            },
            pool: 0,
            slot: None,
            textures: NativeTextures::default(),
            skeleton: None,
            meshes: Arc::new(
                vertex_counts
                    .iter()
                    .enumerate()
                    .map(|(index, &vertices)| LoadedMesh {
                        index,
                        vertices,
                        visible: true,
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn mesh_visibility_is_asset_local_and_retains_old_snapshots() {
        let first = asset_with_meshes(&[415, 84, 381, 83]);
        let mut second = asset_with_meshes(&[415, 84, 381, 83]);
        let original = second.meshes();
        second.set_mesh_visible(1, false).unwrap();
        second.set_mesh_visible(3, false).unwrap();
        assert_eq!(
            second
                .meshes()
                .iter()
                .map(|mesh| mesh.visible)
                .collect::<Vec<_>>(),
            [true, false, true, false]
        );
        assert!(first.meshes().iter().all(|mesh| mesh.visible));
        assert!(original.iter().all(|mesh| mesh.visible));
        second.isolate_mesh(3).unwrap();
        assert_eq!(
            second
                .meshes()
                .iter()
                .filter(|mesh| mesh.visible)
                .map(|mesh| mesh.index)
                .collect::<Vec<_>>(),
            [3]
        );
        second.show_all_meshes();
        assert_eq!(second.meshes(), original);
        assert_eq!(second.requirements.mesh_vertices, [415, 84, 381, 83]);
    }

    #[test]
    fn invalid_mesh_indices_leave_visibility_and_snapshot_ownership_unchanged() {
        let mut asset = asset_with_meshes(&[10, 20, 30]);
        asset.isolate_mesh(1).unwrap();
        let before = asset.meshes();
        for index in [3, usize::MAX] {
            assert!(asset.set_mesh_visible(index, false).is_err());
            assert!(asset.isolate_mesh(index).is_err());
            assert!(Arc::ptr_eq(&before, &asset.meshes()));
        }
        assert_eq!(
            asset
                .meshes()
                .iter()
                .map(|mesh| mesh.visible)
                .collect::<Vec<_>>(),
            [false, true, false]
        );
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn actual_resource_bundles_reach_native_preflight() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let mut supported = 0;
        for name in [
            "dat/npc/npc41.bin",
            "dat/npc/n37f0050.bin",
            "dat/emmodel/em019.pac",
            "dat/emmodel/em001.pac",
            "dat/emmodel-hd/em001-hd.pac",
            "dat/emmodel-hd/em002_c-hd.pac",
            "dat/emmodel-hd/em019-hd.pac",
            "dat/emmodel-hd/em077_b-hd.pac",
            "dat/emmodel-hd/em150-hd.pac",
            "dat/parts/m00/m_editpl.bin",
            "dat/weapon/wi127-so.bin",
            "dat/mytra.bin",
            "dat/extend/f00_body.abn",
            "dat/extend/f01_wst.abn",
        ] {
            let bytes = std::fs::read(root.join(name)).unwrap();
            let document = std::sync::Arc::new(crate::inspect::inspect(name, bytes.into()));
            let bundles = AssetBundle::find_with_nodes(document.clone()).0;
            eprintln!("{name}: {} bundles", bundles.len());
            let required = matches!(
                name,
                "dat/emmodel/em001.pac"
                    | "dat/emmodel-hd/em001-hd.pac"
                    | "dat/emmodel-hd/em002_c-hd.pac"
                    | "dat/emmodel-hd/em150-hd.pac"
                    | "dat/mytra.bin"
            );
            let single_image = matches!(name, "dat/extend/f00_body.abn" | "dat/extend/f01_wst.abn");
            if single_image {
                assert_eq!(
                    bundles.len(),
                    if name == "dat/extend/f00_body.abn" {
                        736
                    } else {
                        798
                    },
                    "{name}: every nonempty member has model, skeleton and standalone PNG"
                );
            }
            if required {
                assert_eq!(bundles.len(), 2, "{name}: primary and secondary model");
            }
            assert_eq!(
                bundles.len(),
                document
                    .nodes
                    .iter()
                    .filter(|node| node.kind == crate::inspect::Kind::Fmod)
                    .count(),
                "all geometry members must have their matching texture bank: {name}"
            );
            let mut ready = 0;
            let mut external_image = 0;
            // Explicit test binding to the shared source found through
            // 108E25D0; runtime attachment never uses this filename rule.
            let shared_skin = (name == "dat/extend/f00_body.abn").then(|| {
                let source = std::fs::read(root.join("dat/parts/f00/f_skin.txb")).unwrap();
                let document = Arc::new(crate::inspect::inspect("f_skin.txb", source.into()));
                crate::preview::ResourceRef {
                    node: document.root,
                    document,
                }
            });
            let mut supplied = 0;
            for bundle in bundles {
                let result = preflight(&bundle);
                if !single_image {
                    eprintln!("{}: {result:?}", bundle.name);
                }
                if single_image {
                    let source = bundle.textures[0].bytes().unwrap();
                    let images = textures::images(&[source]).unwrap();
                    assert!(matches!(&images[..], [Image::Png(_)]));
                    assert_eq!(images[0].as_bytes().as_ptr(), source.as_ptr());
                    assert_eq!(images[0].as_bytes(), source);
                    if let Err(error) = &result {
                        assert!(
                            name == "dat/extend/f00_body.abn" && error.contains("image ID"),
                            "{}: {result:?}",
                            bundle.name
                        );
                        external_image += 1;
                    }
                    if let Some(skin) = &shared_skin {
                        let mut complete = bundle.clone();
                        complete.textures.push(skin.clone());
                        preflight(&complete).unwrap_or_else(|error| {
                            panic!("{} + shared skin: {error}", bundle.name)
                        });
                        let sources = complete
                            .textures
                            .iter()
                            .map(|source| source.bytes().unwrap())
                            .collect::<Vec<_>>();
                        let images = textures::images(&sources).unwrap();
                        let skin_images = textures::images(&[skin.bytes().unwrap()]).unwrap();
                        assert_eq!(images.len(), 1 + skin_images.len());
                        assert_eq!(images[0].as_bytes().as_ptr(), source.as_ptr());
                        assert_eq!(
                            images[1].as_bytes().as_ptr(),
                            skin_images[0].as_bytes().as_ptr()
                        );
                        assert_ne!(images[1].as_bytes().as_ptr(), source.as_ptr());
                        supplied += 1;
                    }
                } else if required {
                    assert!(result.is_ok(), "{}: {result:?}", bundle.name);
                }
                if name == "dat/weapon/wi127-so.bin" {
                    let requirements = validate_files(
                        bundle.model.bytes().unwrap(),
                        bundle
                            .skeleton
                            .as_ref()
                            .map(|source| source.bytes().unwrap()),
                        &[bundle.textures[0].bytes().unwrap()],
                    )
                    .unwrap();
                    assert_eq!(requirements.mesh_vertices, [415, 84, 381, 83]);
                }
                ready += usize::from(result.is_ok());
            }
            if single_image {
                let expected = if name == "dat/extend/f00_body.abn" {
                    (77, 659)
                } else {
                    (798, 0)
                };
                assert_eq!((ready, external_image), expected);
                eprintln!(
                    "{name}: {ready} self-contained PNG previews; {external_image} require an external image slot"
                );
                if shared_skin.is_some() {
                    assert_eq!(supplied, 736);
                    eprintln!(
                        "{name}: all {supplied} models pass with explicit primary PNG + shared skin inputs; image 1 retains the shared source"
                    );
                }
            }
            supported += ready;
        }
        assert!(
            supported > 0,
            "no actual source bundle passed native preflight"
        );
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn actual_skin_palettes_support_node_ids_beyond_the_old_cache() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let name = "dat/emmodel-hd/em001-hd.pac";
        let document = std::sync::Arc::new(crate::inspect::inspect(
            name,
            std::fs::read(root.join(name)).unwrap().into(),
        ));
        let bundles = AssetBundle::find_with_nodes(document).0;
        let bundle = &bundles[0];
        let mut model = bundle.model.bytes().unwrap().to_vec();
        let mut skeleton = bundle.skeleton.as_ref().unwrap().bytes().unwrap().to_vec();
        let before = validate_files(
            &model,
            Some(&skeleton),
            &[bundle.textures[0].bytes().unwrap()],
        )
        .unwrap();
        let node_offsets: Vec<_> = Fskl::parse(&skeleton)
            .unwrap()
            .bones()
            .map(|bone| (bone.block.offset() + 12, bone.node_id as u32))
            .collect();
        for (offset, id) in node_offsets {
            skeleton[offset..offset + 4].copy_from_slice(&(id + 1000).to_le_bytes());
        }
        let fmod = Fmod::parse(&model).unwrap();
        let mut map_offsets = Vec::new();
        for section in &fmod.sections {
            if let Section::Meshes(meshes) = section {
                for object in &meshes.entries {
                    if let ObjectEntry::Object(object) = object {
                        for component in &object.components {
                            if let Component::BoneMap(map) = component {
                                map_offsets.extend(
                                    map.values
                                        .iter()
                                        .enumerate()
                                        .map(|(i, &id)| (map.block.offset() + 12 + 4 * i, id)),
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(!map_offsets.is_empty());
        for (offset, id) in map_offsets {
            model[offset..offset + 4].copy_from_slice(&(id + 1000).to_le_bytes());
        }
        let after = validate_files(
            &model,
            Some(&skeleton),
            &[bundle.textures[0].bytes().unwrap()],
        )
        .unwrap();
        assert!(after.node_ids.iter().all(|&id| id >= 1000));
        assert_eq!(
            before.bone_maps, after.bone_maps,
            "GPU slots retain the same source nodes after ID remapping"
        );
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn native_consumption_preserves_root_layout_channels_weights_and_empty_texture_banks() {
        use mhf_resource::container::{SimpleArchive, open_layers};
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let bytes = std::fs::read(root.join("dat/emmodel-hd/em001-hd.pac")).unwrap();
        let opened = open_layers(&bytes, usize::MAX, usize::MAX).unwrap();
        let package = SimpleArchive::parse(opened.payload(), usize::MAX).unwrap();
        let model = open_layers(package.payload(0).unwrap(), usize::MAX, usize::MAX).unwrap();
        let members = SimpleArchive::parse(model.payload(), usize::MAX).unwrap();
        let model_source =
            open_layers(members.payload(0).unwrap(), usize::MAX, usize::MAX).unwrap();
        let skeleton_source =
            open_layers(members.payload(1).unwrap(), usize::MAX, usize::MAX).unwrap();
        let source_model = model_source.payload();
        let source_skeleton = skeleton_source.payload();
        let textures = open_layers(package.payload(1).unwrap(), usize::MAX, usize::MAX).unwrap();
        let before =
            validate_files(source_model, Some(source_skeleton), &[textures.payload()]).unwrap();

        let skeleton = Fskl::parse(source_skeleton).unwrap();
        let root_table = skeleton.root_tables[0].block;
        assert_eq!(root_table.offset(), 12);
        let end = root_table.offset() + root_table.as_bytes().len();
        let reordered = [
            &source_skeleton[..12],
            &source_skeleton[end..skeleton.root.as_bytes().len()],
            root_table.as_bytes(),
            skeleton.trailing,
        ]
        .concat();
        let after = validate_files(source_model, Some(&reordered), &[textures.payload()]).unwrap();
        assert_eq!(before.node_ids, after.node_ids);
        assert_eq!(before.bone_maps, after.bone_maps);

        let parsed = Fmod::parse(source_model).unwrap();
        let table = parsed
            .sections
            .iter()
            .find_map(|section| match section {
                Section::Materials(table) => Some(table),
                _ => None,
            })
            .unwrap();
        let MaterialEntry::Material(material) = &table.records[0] else {
            panic!("material")
        };
        let count = material.texture_indices.len();
        assert!((1..=4).contains(&count));
        let mut model = source_model.to_vec();
        let references = material.block.offset() + 12 + 256;
        let first = material.texture_indices[0];
        let five = [first; 5]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        model.splice(references..references + count * 4, five);
        let added = ((5 - count) * 4) as u32;
        for block in [parsed.root, table.block, material.block] {
            let offset = block.offset() + 8;
            model[offset..offset + 4].copy_from_slice(&(block.header.size + added).to_le_bytes());
        }
        let count_offset = material.block.offset() + 12 + 0x34;
        model[count_offset..count_offset + 4].copy_from_slice(&5_u32.to_le_bytes());
        for extra in [first, u32::MAX] {
            model[references + 16..references + 20].copy_from_slice(&extra.to_le_bytes());
            let after =
                validate_files(&model, Some(source_skeleton), &[textures.payload()]).unwrap();
            assert_eq!(after.materials, before.materials);
            let prepared = NativeTextures::prepare(&model, before.textures).unwrap();
            let preview = unsafe {
                std::slice::from_raw_parts(prepared.model_source() as *const u8, model.len())
            };
            let material_range = material.block.offset()
                ..material.block.offset() + material.block.as_bytes().len() + added as usize;
            assert_eq!(
                &preview[material_range.clone()],
                &model[material_range],
                "extra source channels stay unchanged"
            );
        }

        let weight_offset = parsed
            .objects()
            .find_map(|object| {
                object
                    .weights()?
                    .vertices
                    .iter()
                    .find(|vertex| !vertex.influences.is_empty())
                    .map(|vertex| vertex.offset + 8)
            })
            .unwrap();
        let mut weighted = source_model.to_vec();
        for weight in [
            -f32::EPSILON,
            100.0,
            f32::from_bits(0x42c8_0001),
            f32::from_bits(0x42c8_0002),
            101.0,
            f32::MAX,
        ] {
            weighted[weight_offset..weight_offset + 4].copy_from_slice(&weight.to_le_bytes());
            validate_files(&weighted, Some(source_skeleton), &[textures.payload()]).unwrap();
            let prepared = NativeTextures::prepare(&weighted, before.textures).unwrap();
            let preview = unsafe {
                std::slice::from_raw_parts(prepared.model_source() as *const u8, weighted.len())
            };
            assert_eq!(
                &preview[weight_offset..weight_offset + 4],
                &weight.to_le_bytes()
            );
        }
        for weight in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            weighted[weight_offset..weight_offset + 4].copy_from_slice(&weight.to_le_bytes());
            assert!(
                validate_files(&weighted, Some(source_skeleton), &[textures.payload()]).is_err()
            );
        }

        // Make an untextured preview copy: keep every material record and its
        // trailing reference words, setting only the consumed counts to zero.
        let mut untextured = source_model.to_vec();
        for material in &table.records {
            let MaterialEntry::Material(material) = material else {
                panic!("material")
            };
            let offset = material.block.offset() + 12 + 0x34;
            untextured[offset..offset + 4].copy_from_slice(&0_u32.to_le_bytes());
        }
        let images = parsed
            .sections
            .iter()
            .find_map(|section| match section {
                Section::Textures(images) => Some(images.block),
                _ => None,
            })
            .unwrap();
        let start = images.offset();
        let removed = images.header.size - 12;
        untextured.drain(start + 12..start + images.header.size as usize);
        untextured[start + 4..start + 8].copy_from_slice(&0_u32.to_le_bytes());
        untextured[start + 8..start + 12].copy_from_slice(&12_u32.to_le_bytes());
        untextured[8..12].copy_from_slice(&(parsed.root.header.size - removed).to_le_bytes());
        let empty_txb = 0_u32.to_le_bytes();
        assert!(validate_files(&untextured, Some(source_skeleton), &[&empty_txb]).is_err());
        let complete = validate_files(
            &untextured,
            Some(source_skeleton),
            &[&empty_txb, textures.payload()],
        )
        .unwrap();
        assert_eq!(complete.textures, before.textures);
        assert_eq!(complete.materials, before.materials);
        let prepared = NativeTextures::prepare(&untextured, complete.textures).unwrap();
        let preview = unsafe {
            std::slice::from_raw_parts(prepared.model_source() as *const u8, untextured.len())
        };
        assert_eq!(preview, untextured);
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn actual_extend_archive_keeps_zero_influence_records_and_native_buffer_boundary() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let source = std::fs::read(root.join("dat/extend/wf500.abn")).unwrap();
        let document = Arc::new(crate::inspect::inspect(
            "zero-influence regression",
            source.into(),
        ));
        let bundles = AssetBundle::find_with_nodes(document).0;
        assert_eq!(bundles.len(), 400);
        let mut zero_models = 0;
        let mut zero_vertices = 0;
        let mut checked_overflow = false;
        let mut checked_implicit_slot = false;
        for bundle in &bundles {
            let model = bundle.model.bytes().unwrap();
            let skeleton = bundle
                .skeleton
                .as_ref()
                .map(|source| source.bytes().unwrap());
            let textures = bundle.textures[0].bytes().unwrap();
            let requirements = validate_files(model, skeleton, &[textures])
                .unwrap_or_else(|error| panic!("{}: {error}", bundle.name));
            let parsed = Fmod::parse(model).unwrap();
            let mut model_zero_count = 0;
            for (object_index, object) in parsed.objects().enumerate() {
                let Some(weights) = object.weights() else {
                    continue;
                };
                for vertex in &weights.vertices {
                    if vertex.influences.is_empty() {
                        model_zero_count += 1;
                        assert!(
                            requirements.bone_maps[object_index]
                                .iter()
                                .any(|&(slot, _)| slot == 0)
                        );
                    }
                }
                if checked_overflow
                    || !weights
                        .vertices
                        .iter()
                        .any(|vertex| vertex.influences.is_empty())
                {
                    continue;
                }
                // The actual load prepares an owned FMOD copy for texture
                // binding. Its zero-count weight records must remain verbatim.
                let prepared = NativeTextures::prepare(model, requirements.textures).unwrap();
                let prepared_bytes = unsafe {
                    std::slice::from_raw_parts(prepared.model_source() as *const u8, model.len())
                };
                let prepared_model = Fmod::parse(prepared_bytes).unwrap();
                assert_eq!(
                    prepared_model
                        .objects()
                        .nth(object_index)
                        .unwrap()
                        .weights()
                        .unwrap()
                        .block
                        .as_bytes(),
                    weights.block.as_bytes(),
                );

                if parsed.objects().all(|object| {
                    !object
                        .components
                        .iter()
                        .any(|component| matches!(component, Component::BoneMap(_)))
                        && object.weights().is_none_or(|weights| {
                            weights.vertices.iter().all(|vertex| {
                                vertex
                                    .influences
                                    .iter()
                                    .all(|influence| influence.bone_index != 0)
                            })
                        })
                }) {
                    let original = skeleton.unwrap();
                    let parsed_skeleton = Fskl::parse(original).unwrap();
                    let root = parsed_skeleton
                        .bones()
                        .find(|bone| bone.node_id == 0)
                        .unwrap();
                    assert!(
                        parsed_skeleton
                            .bones()
                            .all(|bone| bone.node_id != i32::from(u16::MAX))
                    );
                    let mut without_zero = original.to_vec();
                    let offset = root.block.offset() + 12;
                    without_zero[offset..offset + 4]
                        .copy_from_slice(&u32::from(u16::MAX).to_le_bytes());
                    Fskl::parse(&without_zero)
                        .unwrap()
                        .validate_hierarchy()
                        .unwrap();
                    // Every explicit influence still binds, but the implicit
                    // slot 0 is absent. Reject this instead of reusing a stale
                    // shader matrix or guessing a replacement root.
                    assert!(validate_files(model, Some(&without_zero), &[textures]).is_err());
                    checked_implicit_slot = true;
                }

                // Turn one zero-count vertex into five valid 20% influences
                // in a private copy, updating all containing block lengths.
                // Parsing must succeed, while native preflight still rejects
                // the genuine four-entry stack-buffer overflow.
                let vertex = weights
                    .vertices
                    .iter()
                    .find(|vertex| vertex.influences.is_empty())
                    .unwrap();
                let mut edited = model.to_vec();
                let influence = [0_u32.to_le_bytes(), 20.0_f32.to_le_bytes()].concat();
                edited.splice(vertex.offset + 4..vertex.offset + 4, influence.repeat(5));
                edited[vertex.offset..vertex.offset + 4].copy_from_slice(&5_u32.to_le_bytes());
                let main = parsed
                    .sections
                    .iter()
                    .find_map(|section| match section {
                        Section::Meshes(meshes) => Some(meshes.block),
                        _ => None,
                    })
                    .unwrap();
                for block in [parsed.root, main, object.block, weights.block] {
                    let offset = block.offset() + 8;
                    edited[offset..offset + 4]
                        .copy_from_slice(&(block.header.size + 40).to_le_bytes());
                }
                let overflow = Fmod::parse(&edited).unwrap();
                assert!(
                    overflow
                        .objects()
                        .nth(object_index)
                        .unwrap()
                        .weights()
                        .unwrap()
                        .vertices
                        .iter()
                        .any(|vertex| vertex.influences.len() == 5)
                );
                assert!(validate_files(&edited, skeleton, &[textures]).is_err());
                checked_overflow = true;
            }
            zero_models += usize::from(model_zero_count != 0);
            zero_vertices += model_zero_count;
        }
        eprintln!(
            "wf500.abn: {} bundles, {zero_models} zero-influence models, {zero_vertices} zero-influence vertices",
            bundles.len()
        );
        assert_eq!(zero_models, 19);
        assert!(zero_vertices > 0);
        assert!(checked_overflow);
        assert!(checked_implicit_slot);
    }
}
