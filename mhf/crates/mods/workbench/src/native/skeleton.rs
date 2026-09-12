//! Asset-owned traversal and matrices for the client's 448-byte runtime nodes.
//!
//! 100092A0 keeps 64 pending child lists on its stack; 10008AB0 writes a global
//! matrix array indexed by a signed node ID. Neither limit belongs to FSKL.
//! Keep their transform arithmetic, but size traversal/storage to this resource.

use super::{Client, animation, get, put};
use crate::preview::effects::NodeTransform;
use std::{cmp::Reverse, collections::BinaryHeap, mem::transmute, sync::Arc};

const NODE_SIZE: usize = 448;
const NODE_LOCAL: usize = 64;
const NODE_INVERSE_BIND: usize = 128;
const NODE_ID: usize = 196;
const NODE_PARENT: usize = 200;
const NODE_SIBLING: usize = 204;
const NODE_CHILD: usize = 208;
const NODE_SCALE: usize = 260;
const MATRIX_MULTIPLY_IMPORT: usize = 0x115d_2408;

type Matrix = [f32; 16];

#[derive(Clone, Copy, Debug, Default)]
struct Links {
    parent: Option<usize>,
    sibling: Option<usize>,
    child: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    index: usize,
    parent: Option<usize>,
}

#[derive(Clone, Copy)]
struct Pose {
    local: Matrix,
    inverse_bind: Matrix,
    scale: [f32; 3],
}

pub(super) struct Skeleton {
    nodes: usize,
    original_order: Vec<Step>,
    order: Vec<Step>,
    /// Sorted by raw WORD ID; values index the native allocation, not FSKL.
    ids: Vec<(u16, usize)>,
    worlds: Vec<Matrix>,
    skin: Vec<Matrix>,
    scales: Vec<[f32; 3]>,
    bindings: Arc<Vec<Option<usize>>>,
}

impl Skeleton {
    /// # Safety
    /// `nodes` is a live native allocation of `count` 448-byte nodes. Retain it
    /// and its unchanged hierarchy until this helper is no longer used.
    pub(super) unsafe fn prepare(roots: &[usize], nodes: (usize, usize)) -> Result<Self, String> {
        let (base, count) = nodes;
        if count > i16::MAX as usize || (base == 0) != (count == 0) {
            return Err("原生骨架节点范围无效".into());
        }
        base.checked_add(count.checked_mul(NODE_SIZE).ok_or("骨架范围长度溢出")?)
            .ok_or("骨架地址范围溢出")?;
        let roots = roots
            .iter()
            .map(|&root| node_index(base, count, root)?.ok_or_else(|| "骨架根节点为空".into()))
            .collect::<Result<Vec<_>, String>>()?;
        let mut links = Vec::with_capacity(count);
        let mut ids = Vec::with_capacity(count);
        for index in 0..count {
            let node = base + index * NODE_SIZE;
            // These fields are always DWORD pointers, including in host tests.
            let link =
                |offset| unsafe { node_index(base, count, get::<u32>(node + offset) as usize) };
            links.push(Links {
                // Native 100095D0 only writes parent for non-root nodes;
                // 10009640 does not clear this slot when a pool entry is reused.
                parent: if roots.contains(&index) {
                    None
                } else {
                    link(NODE_PARENT)?
                },
                sibling: link(NODE_SIBLING)?,
                child: link(NODE_CHILD)?,
            });
            ids.push((unsafe { get::<u16>(node + NODE_ID) }, index));
        }
        let skeleton = Self::from_links(base, &roots, &links, ids)?;
        for root in roots {
            unsafe {
                put(base + root * NODE_SIZE + NODE_PARENT, 0u32);
            }
        }
        Ok(skeleton)
    }

    fn from_links(
        nodes: usize,
        roots: &[usize],
        links: &[Links],
        mut ids: Vec<(u16, usize)>,
    ) -> Result<Self, String> {
        let count = links.len();
        let order = traversal(roots, links)?;
        ids.sort_unstable_by_key(|&(id, _)| id);
        if ids.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err("原生骨架包含重复节点 ID，无法映射蒙皮矩阵".into());
        }
        Ok(Self {
            nodes,
            original_order: order.clone(),
            order,
            ids,
            worlds: vec![[0.0; 16]; count],
            skin: vec![[0.0; 16]; count],
            scales: vec![[1.0; 3]; count],
            bindings: Arc::new(vec![None; count]),
        })
    }

    /// Replace only this node's pose dependency. Its children retain their
    /// original local transforms and follow the copied world pose and scale.
    /// Validate the whole resulting graph before replacing the current state.
    pub(super) fn set_binding(
        &mut self,
        target: usize,
        source: Option<usize>,
    ) -> Result<(), String> {
        if target >= self.bindings.len()
            || source.is_some_and(|source| source >= self.bindings.len())
        {
            return Err("姿态跟随节点超出当前骨架范围".into());
        }
        if source == Some(target) {
            return Err("骨架节点不能跟随自身".into());
        }
        if self.bindings[target] == source {
            return Ok(());
        }
        let mut bindings = self.bindings.as_ref().clone();
        bindings[target] = source;
        let order = binding_order(&self.original_order, &bindings)?;
        self.bindings = Arc::new(bindings);
        self.order = order;
        Ok(())
    }

    pub(super) fn clear_bindings(&mut self) {
        if self.bindings.iter().all(Option::is_none) {
            return;
        }
        self.bindings = Arc::new(vec![None; self.bindings.len()]);
        self.order.clone_from(&self.original_order);
    }

    pub(super) fn bindings(&self) -> Arc<Vec<Option<usize>>> {
        Arc::clone(&self.bindings)
    }

    pub(super) fn node_index(&self, id: u16) -> Option<usize> {
        self.ids
            .binary_search_by_key(&id, |&(id, _)| id)
            .ok()
            .map(|index| self.ids[index].1)
    }

    pub(super) fn worlds(&self) -> &[Matrix] {
        &self.worlds
    }

    pub(super) fn skin_matrices(&self) -> &[Matrix] {
        &self.skin
    }

    /// # Safety
    /// Render phase only, while the allocation supplied to `prepare` and the
    /// supported client DLL remain live, with no concurrent pose/load/release.
    pub(super) unsafe fn update(
        &mut self,
        client: Client,
        frame: f32,
        world: &Matrix,
        transforms: &[NodeTransform],
    ) -> Result<&[Matrix], String> {
        if !frame.is_finite() {
            return Err("预览动画帧坐标无效".into());
        }
        let address = unsafe { client.read::<usize>(MATRIX_MULTIPLY_IMPORT) };
        if address == 0 {
            return Err("原生 D3DX 矩阵乘法接口为空".into());
        }
        let multiply: unsafe extern "system" fn(*mut f32, *const f32, *const f32) -> *mut f32 =
            unsafe { transmute(address) };
        let base = self.nodes;
        self.update_matrices(
            world,
            |index| {
                let node = base + index * NODE_SIZE;
                unsafe {
                    animation::sample(client, node, frame);
                    // Modify the sampled copy, never the original local matrix.
                    let mut local: Matrix = get(node + NODE_LOCAL);
                    if let Some(transform) =
                        transforms.iter().find(|transform| transform.node == index)
                    {
                        transform.apply(&mut local);
                    }
                    Pose {
                        local,
                        inverse_bind: get(node + NODE_INVERSE_BIND),
                        scale: get(node + NODE_SCALE),
                    }
                }
            },
            |left, right| {
                let mut result = [0.0; 16];
                unsafe {
                    multiply(result.as_mut_ptr(), left.as_ptr(), right.as_ptr());
                }
                result
            },
        )?;
        // Bone overlays and other readers use the same node +0 world matrix
        // that 100092A0 produced. Publish only after the full forest succeeds.
        for (index, matrix) in self.worlds.iter().enumerate() {
            unsafe {
                put(base + index * NODE_SIZE, *matrix);
            }
        }
        Ok(&self.skin)
    }

    fn update_matrices(
        &mut self,
        world: &Matrix,
        mut pose: impl FnMut(usize) -> Pose,
        mut multiply: impl FnMut(&Matrix, &Matrix) -> Matrix,
    ) -> Result<(), String> {
        if !world.iter().all(|value| value.is_finite()) {
            return Err("预览世界矩阵无效".into());
        }
        for &Step { index, parent } in &self.order {
            let pose = pose(index);
            let binding = self.bindings[index];
            let inherited = binding
                .or(parent)
                .map_or([1.0; 3], |parent| self.scales[parent]);
            let scale = if binding.is_some() {
                inherited
            } else {
                std::array::from_fn(|axis| pose.scale[axis] * inherited[axis])
            };
            if scale.iter().any(|value| !value.is_finite()) {
                return Err(format!("骨架节点 {index} 的累计缩放无法用于矩阵运算"));
            }
            let node_world = if let Some(source) = binding {
                self.worlds[source]
            } else {
                let reciprocal = inherited.map(|value| 1.0 / value);
                let mut local = [0.0; 16];
                // 10009390..100094E6: multiply by the accumulated row scale
                // first, then the reciprocal parent column scale. Preserve
                // the sampled local matrix's existing S*R behavior.
                for row in 0..3 {
                    for column in 0..3 {
                        local[row * 4 + column] =
                            (pose.local[row * 4 + column] * scale[row]) * reciprocal[column];
                    }
                }
                local[12..15].copy_from_slice(&pose.local[12..15]);
                local[15] = 1.0;
                let parent_world = parent.map_or(world, |parent| &self.worlds[parent]);
                multiply(&local, parent_world)
            };
            // A copied world pose still uses this target's own inverse bind;
            // copying the source skin matrix would cancel the wrong bind pose.
            let skin = multiply(&pose.inverse_bind, &node_world);
            if node_world
                .iter()
                .chain(&skin)
                .any(|value| !value.is_finite())
            {
                return Err(format!("骨架节点 {index} 的世界或蒙皮矩阵包含非有限值"));
            }
            self.scales[index] = scale;
            self.worlds[index] = node_world;
            self.skin[index] = skin;
        }
        Ok(())
    }
}

fn binding_order(original: &[Step], bindings: &[Option<usize>]) -> Result<Vec<Step>, String> {
    if bindings.iter().all(Option::is_none) {
        return Ok(original.to_vec());
    }
    let mut positions = vec![0; original.len()];
    let mut dependents = vec![Vec::new(); original.len()];
    let mut ready = BinaryHeap::new();
    for (position, &Step { index, parent }) in original.iter().enumerate() {
        positions[index] = position;
        if let Some(source) = bindings[index].or(parent) {
            dependents[source].push(index);
        } else {
            ready.push(Reverse(position));
        }
    }
    let mut order = Vec::with_capacity(original.len());
    while let Some(Reverse(position)) = ready.pop() {
        let step = original[position];
        order.push(step);
        for &index in &dependents[step.index] {
            // Every node depends on exactly one source or original parent.
            ready.push(Reverse(positions[index]));
        }
    }
    if order.len() != original.len() {
        return Err("姿态跟随与原有父子关系形成循环，未修改绑定".into());
    }
    Ok(order)
}

fn node_index(base: usize, count: usize, address: usize) -> Result<Option<usize>, String> {
    if address == 0 {
        return Ok(None);
    }
    let offset = address
        .checked_sub(base)
        .ok_or("骨架节点指针低于资源范围")?;
    if !offset.is_multiple_of(NODE_SIZE) || offset / NODE_SIZE >= count {
        return Err("骨架节点指针超出资源范围或未对齐节点边界".into());
    }
    Ok(Some(offset / NODE_SIZE))
}

fn traversal(roots: &[usize], links: &[Links]) -> Result<Vec<Step>, String> {
    let mut seen = vec![false; links.len()];
    let mut order = Vec::with_capacity(links.len());
    let mut pending = Vec::new();
    for &root in roots {
        pending.push(Step {
            index: root,
            parent: None,
        });
        while let Some(Step { mut index, parent }) = pending.pop() {
            // Match 100092A0: finish this sibling list, then process queued
            // child lists in reverse order. Every parent precedes its children.
            loop {
                let node = links.get(index).ok_or("骨架节点索引越界")?;
                if seen[index] {
                    return Err("骨架包含重复根、共享节点或循环引用".into());
                }
                if node.parent != parent {
                    return Err("骨架父节点与子节点或兄弟链接不一致".into());
                }
                seen[index] = true;
                order.push(Step { index, parent });
                if let Some(child) = node.child {
                    pending.push(Step {
                        index: child,
                        parent: Some(index),
                    });
                }
                match node.sibling {
                    Some(sibling) => index = sibling,
                    None => break,
                }
            }
        }
    }
    if seen.iter().any(|seen| !seen) {
        return Err("原生骨架存在未归属任何根的节点".into());
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY: Matrix = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    fn multiply(left: &Matrix, right: &Matrix) -> Matrix {
        std::array::from_fn(|index| {
            (0..4)
                .map(|axis| left[index / 4 * 4 + axis] * right[axis * 4 + index % 4])
                .sum()
        })
    }

    fn fixture(roots: &[usize], links: &[Links]) -> Skeleton {
        Skeleton::from_links(
            0,
            roots,
            links,
            (0..links.len())
                .map(|index| (1000 + index as u16, index))
                .collect(),
        )
        .unwrap()
    }

    fn translated_pose(x: f32) -> Pose {
        let mut local = IDENTITY;
        local[12] = x;
        Pose {
            local,
            inverse_bind: IDENTITY,
            scale: [1.0; 3],
        }
    }

    #[test]
    fn evaluates_deep_and_wide_forests_beyond_64_nodes() {
        let chain: Vec<_> = (0usize..257)
            .map(|index| Links {
                parent: index.checked_sub(1),
                child: (index < 256).then_some(index + 1),
                sibling: None,
            })
            .collect();
        let mut skeleton = fixture(&[0], &chain);
        skeleton
            .update_matrices(&IDENTITY, |_| translated_pose(1.0), multiply)
            .unwrap();
        assert_eq!(skeleton.worlds[256][12], 257.0);
        assert_eq!(skeleton.node_index(1256), Some(256));
        assert_eq!(skeleton.node_index(999), None);

        // 88 siblings each have a child: the old traversal simultaneously kept
        // 88 pending lists in its 64-entry stack, despite the tree's depth of 3.
        let mut wide = vec![Links::default(); 177];
        wide[0].child = Some(1);
        for index in 1..=88 {
            wide[index] = Links {
                parent: Some(0),
                sibling: (index < 88).then_some(index + 1),
                child: Some(index + 88),
            };
            wide[index + 88].parent = Some(index);
        }
        let mut skeleton = fixture(&[0], &wide);
        skeleton
            .update_matrices(&IDENTITY, |_| translated_pose(1.0), multiply)
            .unwrap();
        assert_eq!(skeleton.skin.len(), 177);
        for index in 1..=88 {
            assert_eq!(skeleton.worlds[index][12], 2.0);
            assert_eq!(skeleton.worlds[index + 88][12], 3.0);
        }
        assert_eq!(skeleton.order[89].index, 176);
    }

    // Independent transcription of the old sibling-list loop and fixed pending
    // array. Only used on a small forest where the original stack is sufficient.
    fn legacy_worlds(
        roots: &[usize],
        links: &[Links],
        poses: &[Pose],
        world: Matrix,
    ) -> Vec<Matrix> {
        let mut output = vec![[0.0; 16]; links.len()];
        let mut pending = [(0usize, [1.0; 3]); 64];
        for &root in roots {
            let mut next = Some(root);
            let mut inherited = [1.0; 3];
            let mut parent_world = world;
            let mut pending_count = 0;
            loop {
                while let Some(index) = next {
                    let Pose { local, scale, .. } = poses[index];
                    let x = scale[0] * inherited[0];
                    let y = scale[1] * inherited[1];
                    let z = scale[2] * inherited[2];
                    let ix = 1.0 / inherited[0];
                    let iy = 1.0 / inherited[1];
                    let iz = 1.0 / inherited[2];
                    let adjusted = [
                        (local[0] * x) * ix,
                        (local[1] * x) * iy,
                        (local[2] * x) * iz,
                        0.0,
                        (local[4] * y) * ix,
                        (local[5] * y) * iy,
                        (local[6] * y) * iz,
                        0.0,
                        (local[8] * z) * ix,
                        (local[9] * z) * iy,
                        (local[10] * z) * iz,
                        0.0,
                        local[12],
                        local[13],
                        local[14],
                        1.0,
                    ];
                    output[index] = multiply(&adjusted, &parent_world);
                    if links[index].child.is_some() {
                        pending[pending_count] = (index, [x, y, z]);
                        pending_count += 1;
                    }
                    next = links[index].sibling;
                }
                if pending_count == 0 {
                    break;
                }
                pending_count -= 1;
                let (parent, scale) = pending[pending_count];
                next = links[parent].child;
                inherited = scale;
                parent_world = output[parent];
            }
        }
        output
    }

    #[test]
    fn matches_legacy_world_and_skin_order_with_nonuniform_scale_and_multiple_roots() {
        let links = [
            Links {
                child: Some(2),
                ..Links::default()
            },
            Links {
                parent: Some(0),
                child: Some(3),
                ..Links::default()
            },
            Links {
                parent: Some(0),
                sibling: Some(1),
                ..Links::default()
            },
            Links {
                parent: Some(1),
                ..Links::default()
            },
            Links::default(),
        ];
        let poses: Vec<_> = (0..links.len())
            .map(|index| {
                let mut pose = translated_pose(index as f32 + 0.5);
                // 10009A20 has already formed S*R, including nonuniform scale.
                pose.local[..3].copy_from_slice(&[0.0, 2.0, 0.0]);
                pose.local[4..7].copy_from_slice(&[0.5, 0.0, 0.0]);
                pose.local[10] = 1.25;
                pose.scale = [2.0, -0.5, 1.25];
                pose.inverse_bind[13] = -2.0;
                pose
            })
            .collect();
        let mut world = IDENTITY;
        world[12..15].copy_from_slice(&[11.0, 13.0, 17.0]);
        let expected = legacy_worlds(&[0, 4], &links, &poses, world);
        let mut skeleton = fixture(&[0, 4], &links);
        skeleton
            .update_matrices(&world, |index| poses[index], multiply)
            .unwrap();
        assert_eq!(skeleton.worlds, expected);
        for (index, expected) in expected.iter().enumerate() {
            assert_eq!(
                skeleton.skin[index],
                multiply(&poses[index].inverse_bind, expected)
            );
        }
        // A rotated, nonuniformly scaled parent makes multiplication reversal
        // and accidentally applying parent scale to the translation observable.
        assert_eq!(skeleton.worlds[0][12..15], [11.5, 13.0, 17.0]);
        assert_eq!(skeleton.worlds[0][1], 4.0);
        assert_eq!(skeleton.worlds[0][4], -0.25);
        assert_eq!(skeleton.worlds[0][10], 1.5625);
        assert_eq!(skeleton.worlds[2][12..15], [11.5, 23.0, 17.0]);
        assert_eq!(skeleton.worlds[4][12..15], [15.5, 13.0, 17.0]);
    }

    #[test]
    fn effect_pose_copy_deforms_skin_and_restores_without_accumulating() {
        let mut skeleton = fixture(&[0], &[Links::default()]);
        let original = translated_pose(10.0);
        let effect = NodeTransform {
            mesh: 1,
            node: 0,
            translation: [0.0, 5.0, 0.0],
            rotation: [0.0, 0.0, 90.0],
            scale: [2.0, 1.0, 0.0],
        };
        skeleton
            .update_matrices(&IDENTITY, |_| original, multiply)
            .unwrap();
        let base = skeleton.skin.clone();
        for _ in 0..2 {
            skeleton
                .update_matrices(
                    &IDENTITY,
                    |_| {
                        let mut pose = original;
                        effect.apply(&mut pose.local);
                        pose
                    },
                    multiply,
                )
                .unwrap();
            assert!((skeleton.skin[0][1] - 2.0).abs() < 0.00001);
            assert_eq!(skeleton.worlds[0][13], original.local[13] + 5.0);
            assert_eq!(skeleton.worlds[0][10], 0.0);
        }
        skeleton
            .update_matrices(&IDENTITY, |_| original, multiply)
            .unwrap();
        assert_eq!(skeleton.skin, base);
    }

    #[test]
    fn bindings_follow_later_roots_and_keep_target_inverse_bind_and_child_transforms() {
        let links = [
            Links {
                child: Some(1),
                ..Links::default()
            },
            Links {
                parent: Some(0),
                child: Some(2),
                ..Links::default()
            },
            Links {
                parent: Some(1),
                ..Links::default()
            },
            Links {
                child: Some(4),
                ..Links::default()
            },
            Links {
                parent: Some(3),
                ..Links::default()
            },
        ];
        let mut poses = [2.0, 100.0, 7.0, 20.0, 5.0].map(translated_pose);
        poses[1].inverse_bind[12] = -100.0;
        let mut world = IDENTITY;
        world[12] = 10.0;
        let mut skeleton = fixture(&[0, 3], &links);
        let original_order = skeleton.original_order.clone();
        let original_bindings = skeleton.bindings();
        skeleton.set_binding(1, Some(4)).unwrap();
        let mut samples = [0; 5];
        skeleton
            .update_matrices(
                &world,
                |node| {
                    samples[node] += 1;
                    poses[node]
                },
                multiply,
            )
            .unwrap();
        assert_eq!(samples, [1; 5]);
        assert_eq!(
            skeleton.bindings().as_ref(),
            &[None, Some(4), None, None, None]
        );
        assert!(original_bindings.iter().all(Option::is_none));
        assert!(!Arc::ptr_eq(&original_bindings, &skeleton.bindings()));
        assert_eq!(skeleton.worlds[1], skeleton.worlds[4]);
        assert_eq!(skeleton.worlds[1][12], 35.0);
        assert_eq!(skeleton.skin[1][12], -65.0);
        assert_eq!(skeleton.skin[4][12], 35.0);
        assert_eq!(skeleton.worlds[2][12], 42.0);
        assert_eq!(skeleton.original_order, original_order);

        skeleton.set_binding(0, Some(4)).unwrap();
        skeleton
            .update_matrices(&world, |node| poses[node], multiply)
            .unwrap();
        assert_eq!(
            skeleton.bindings().as_ref(),
            &[Some(4), Some(4), None, None, None]
        );
        assert_eq!(skeleton.worlds[0], skeleton.worlds[4]);
        assert_eq!(skeleton.worlds[1][12], 35.0);
        assert_eq!(skeleton.worlds[2][12], 42.0);
        skeleton.set_binding(1, None).unwrap();
        skeleton
            .update_matrices(&world, |node| poses[node], multiply)
            .unwrap();
        assert_eq!(skeleton.worlds[1][12], 135.0);
        assert_eq!(skeleton.worlds[2][12], 142.0);
        skeleton.clear_bindings();
        skeleton
            .update_matrices(&world, |node| poses[node], multiply)
            .unwrap();
        assert!(skeleton.bindings().iter().all(Option::is_none));
        assert_eq!(skeleton.order, original_order);
        assert_eq!(skeleton.worlds[1][12], 112.0);
        assert_eq!(skeleton.worlds[2][12], 119.0);
    }

    #[test]
    fn bound_nodes_copy_cumulative_scale_for_their_unbound_children() {
        let links = [
            Links::default(),
            Links {
                child: Some(2),
                ..Links::default()
            },
            Links {
                parent: Some(1),
                ..Links::default()
            },
        ];
        let mut poses = [10.0, 999.0, 3.0].map(translated_pose);
        poses[0].scale = [2.0, 3.0, 4.0];
        poses[1].scale = [9.0; 3];
        let mut skeleton = fixture(&[0, 1], &links);
        skeleton.set_binding(1, Some(0)).unwrap();
        skeleton
            .update_matrices(&IDENTITY, |node| poses[node], multiply)
            .unwrap();
        assert_eq!(skeleton.worlds[1], skeleton.worlds[0]);
        assert_eq!(skeleton.scales[1], [2.0, 3.0, 4.0]);
        assert_eq!(skeleton.scales[2], [2.0, 3.0, 4.0]);
        assert_eq!(skeleton.worlds[2][12], 16.0);
        skeleton.set_binding(1, None).unwrap();
        skeleton
            .update_matrices(&IDENTITY, |node| poses[node], multiply)
            .unwrap();
        assert_eq!(skeleton.scales[2], [9.0; 3]);
        assert_eq!(skeleton.worlds[2][12], 1026.0);
    }

    #[test]
    fn invalid_binding_additions_and_removals_leave_the_whole_state_unchanged() {
        let links = [
            Links {
                child: Some(1),
                ..Links::default()
            },
            Links {
                parent: Some(0),
                ..Links::default()
            },
            Links::default(),
        ];
        let mut skeleton = fixture(&[0, 2], &links);
        let original_order = skeleton.order.clone();
        skeleton.set_binding(1, Some(2)).unwrap();
        skeleton.set_binding(0, Some(1)).unwrap();
        let poses = [1.0, 3.0, 7.0].map(translated_pose);
        skeleton
            .update_matrices(&IDENTITY, |node| poses[node], multiply)
            .unwrap();
        assert!(skeleton.worlds.iter().all(|world| world[12] == 7.0));
        let bindings = skeleton.bindings();
        let order = skeleton.order.clone();
        let worlds = skeleton.worlds.clone();
        for (target, source) in [
            (0, Some(0)),
            (3, None),
            (1, Some(3)),
            (usize::MAX, Some(0)),
            (0, Some(usize::MAX)),
            (2, Some(0)),
            // Restoring node 1's original parent 0 creates a cycle because
            // node 0 currently follows node 1 instead of its original root.
            (1, None),
        ] {
            assert!(skeleton.set_binding(target, source).is_err());
            assert!(Arc::ptr_eq(&skeleton.bindings(), &bindings));
            assert_eq!(skeleton.order, order);
            assert_eq!(skeleton.worlds, worlds);
        }
        skeleton.clear_bindings();
        assert_eq!(bindings.as_ref(), &[Some(1), Some(2), None]);
        assert_eq!(skeleton.order, original_order);
        skeleton
            .update_matrices(&IDENTITY, |node| poses[node], multiply)
            .unwrap();
        assert_eq!(
            skeleton
                .worlds
                .iter()
                .map(|world| world[12])
                .collect::<Vec<_>>(),
            [1.0, 4.0, 7.0]
        );
    }

    #[test]
    fn rejects_invalid_pointer_boundaries_and_incomplete_or_inconsistent_forests() {
        assert_eq!(
            node_index(0x1000, 177, 0x1000 + 176 * NODE_SIZE).unwrap(),
            Some(176)
        );
        for address in [0xfff, 0x1001, 0x1000 + 177 * NODE_SIZE] {
            assert!(node_index(0x1000, 177, address).is_err());
        }
        let valid = [
            Links {
                child: Some(1),
                ..Links::default()
            },
            Links {
                parent: Some(0),
                ..Links::default()
            },
        ];
        assert!(traversal(&[0, 0], &valid).is_err());
        assert!(traversal(&[], &valid).is_err());
        let mut bad = valid;
        bad[1].parent = None;
        assert!(traversal(&[0], &bad).is_err());
        bad = valid;
        bad[1].sibling = Some(1);
        assert!(traversal(&[0], &bad).is_err());
        bad = valid;
        bad[0].child = Some(2);
        assert!(traversal(&[0], &bad).is_err());
        assert!(Skeleton::from_links(0, &[0], &valid, vec![(7, 0), (7, 1)]).is_err());
        assert!(unsafe { Skeleton::prepare(&[], (usize::MAX - NODE_SIZE, 2)) }.is_err());
        assert!(
            unsafe { Skeleton::prepare(&[], (0, 0)) }
                .unwrap()
                .skin
                .is_empty()
        );
    }

    #[test]
    fn rejects_invalid_animated_scale_and_world() {
        let mut skeleton = fixture(&[0], &[Links::default()]);
        for scale in [f32::NAN, f32::INFINITY] {
            let mut pose = translated_pose(0.0);
            pose.scale[0] = scale;
            assert!(
                skeleton
                    .update_matrices(&IDENTITY, |_| pose, multiply)
                    .is_err()
            );
        }
        let mut world = IDENTITY;
        world[0] = f32::NAN;
        assert!(
            skeleton
                .update_matrices(&world, |_| translated_pose(0.0), multiply)
                .is_err()
        );
    }

    #[test]
    fn zero_scale_leaf_matches_native_but_zero_parent_cannot_feed_unbound_children() {
        let links = [
            Links {
                child: Some(1),
                ..Links::default()
            },
            Links {
                parent: Some(0),
                ..Links::default()
            },
        ];
        let mut poses = [translated_pose(1.0); 2];
        poses[1].scale[0] = 0.0;
        let mut skeleton = fixture(&[0], &links);
        skeleton
            .update_matrices(&IDENTITY, |index| poses[index], multiply)
            .unwrap();
        assert_eq!(
            skeleton.worlds,
            legacy_worlds(&[0], &links, &poses, IDENTITY)
        );
        assert!(
            skeleton
                .worlds
                .iter()
                .flatten()
                .all(|value| value.is_finite())
        );
        assert_eq!(skeleton.worlds[1][0], 0.0);

        // Native takes the reciprocal of the parent's accumulated scale only
        // when descending into a child list. This case actually generates NaN.
        poses[0].scale[0] = 0.0;
        assert!(
            skeleton
                .update_matrices(&IDENTITY, |index| poses[index], multiply)
                .is_err()
        );
    }

    #[test]
    fn recomputes_every_root_after_a_partial_frame_failure() {
        let links = [
            Links {
                child: Some(1),
                ..Links::default()
            },
            Links {
                parent: Some(0),
                ..Links::default()
            },
            Links::default(),
        ];
        let mut skeleton = fixture(&[0, 2], &links);
        skeleton
            .update_matrices(&IDENTITY, |_| translated_pose(1.0), multiply)
            .unwrap();
        let mut broken = [translated_pose(5.0); 3];
        broken[2].scale[0] = f32::NAN;
        assert!(
            skeleton
                .update_matrices(&IDENTITY, |index| broken[index], multiply)
                .is_err()
        );
        let mut world = IDENTITY;
        world[12] = 30.0;
        skeleton
            .update_matrices(&world, |_| translated_pose(2.0), multiply)
            .unwrap();
        assert_eq!(
            skeleton
                .worlds
                .iter()
                .map(|matrix| matrix[12])
                .collect::<Vec<_>>(),
            [32.0, 34.0, 32.0]
        );
        assert_eq!(skeleton.skin, skeleton.worlds);
    }

    #[test]
    #[cfg(target_pointer_width = "32")]
    fn prepares_native_448_byte_nodes_with_full_word_ids_and_checks_links_before_use() {
        let mut nodes = vec![[0u8; NODE_SIZE]; 177];
        let base = nodes.as_mut_ptr() as usize;
        for index in 0..nodes.len() {
            let node = base + index * NODE_SIZE;
            unsafe {
                // Native pool order need not match source IDs, and IDs are raw
                // WORD values even above the old signed 0..63 scratch range.
                put(node + NODE_ID, 40_000u16 + (176 - index) as u16);
                put(node + NODE_LOCAL, translated_pose(1.0).local);
                put(node + NODE_INVERSE_BIND, IDENTITY);
                put(node + NODE_SCALE, [1.0f32; 3]);
                if index > 0 {
                    put(node + NODE_PARENT, base as u32);
                    if index < 176 {
                        put(node + NODE_SIBLING, (node + NODE_SIZE) as u32);
                    }
                }
            }
        }
        unsafe {
            put(base + NODE_CHILD, (base + NODE_SIZE) as u32);
            // The native pool's root parent slot may belong to its previous user.
            put(base + NODE_PARENT, u32::MAX);
        }
        let mut skeleton = unsafe { Skeleton::prepare(&[base], (base, nodes.len())) }.unwrap();
        assert_eq!(unsafe { get::<u32>(base + NODE_PARENT) }, 0);
        assert_eq!(skeleton.node_index(40_000), Some(176));
        skeleton
            .update_matrices(
                &IDENTITY,
                |index| unsafe {
                    let node = base + index * NODE_SIZE;
                    Pose {
                        local: get(node + NODE_LOCAL),
                        inverse_bind: get(node + NODE_INVERSE_BIND),
                        scale: get(node + NODE_SCALE),
                    }
                },
                multiply,
            )
            .unwrap();
        assert_eq!(skeleton.skin[176][12], 2.0);
        unsafe { put(base + NODE_CHILD, (base + nodes.len() * NODE_SIZE) as u32) };
        let before = nodes.clone();
        assert!(unsafe { Skeleton::prepare(&[base], (base, nodes.len())) }.is_err());
        assert_eq!(nodes, before);
    }
}
