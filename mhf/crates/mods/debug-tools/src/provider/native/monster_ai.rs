//! Explicit, game-thread-only AI inspection and actor-local replacement.
use super::{State, get, monster, put};
use crate::provider::{AiDocument, AiOperation, AiReply, AiTarget, MonsterStatus};
use mhf_monster::ai::{
    self, Base,
    bind::{Arena, NativeMemory},
    decompile::Memory,
};
use std::sync::Arc;
use windows::Win32::System::{Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess};

const DESCRIPTOR: usize = 2544;
const MAX_RETAINED: usize = 32 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Editor {
    pub(super) epoch: u64,
    pub(super) reply: Option<Arc<AiReply>>,
    pool: u32,
    // Old continuations may still be referenced by native state. Keep every
    // published allocation until the debug provider's native callers stop.
    retained: Vec<Blocks>,
    retained_bytes: usize,
    originals: Vec<(AiTarget, u32)>,
}

impl Editor {
    pub(super) fn observe(&mut self, pool: u32) {
        if self.pool != pool {
            self.invalidate();
        }
        self.pool = pool;
    }

    pub(super) fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.reply = None;
        self.originals.clear();
    }
}

pub(super) unsafe fn targets(state: &State, epoch: u64) -> Vec<AiTarget> {
    unsafe { pool_targets(state.read::<usize>(monster::POOL), epoch) }
}

/// Called on the game thread immediately after collecting live targets.
pub(super) unsafe fn statuses(targets: &[AiTarget]) -> Vec<MonsterStatus> {
    targets
        .iter()
        .map(|&target| {
            let actor = target.pool as usize + usize::from(target.slot) * monster::STRIDE;
            unsafe {
                MonsterStatus {
                    target,
                    ai_state: get(actor + 2576),
                    action_group: get(actor + 21),
                    action_id: get(actor + 20),
                    action_stage: get(actor + 5),
                    animation: get(actor + 812),
                    frame: get(actor + 476),
                    position: [get(actor + 172), get(actor + 176), get(actor + 180)],
                }
            }
        })
        .collect()
}

unsafe fn pool_targets(pool: usize, epoch: u64) -> Vec<AiTarget> {
    unsafe {
        if pool == 0 {
            return Vec::new();
        }
        (0..monster::SLOTS)
            .filter_map(|slot| {
                let actor = pool + slot * monster::STRIDE;
                let model = get::<u32>(actor + 1656);
                (get::<u8>(actor) != 0 && get::<u32>(actor + DESCRIPTOR) != 0).then(|| AiTarget {
                    epoch,
                    pool: pool as u32,
                    slot: slot as u16,
                    serial: get(actor + 3448),
                    model,
                    species: get(actor + 3),
                })
            })
            .collect()
    }
}

pub(super) unsafe fn execute(
    state: &State,
    editor: &mut Editor,
    target: AiTarget,
    operation: AiOperation,
) -> Result<AiDocument, String> {
    if !unsafe { targets(state, editor.epoch) }.contains(&target) {
        return Err("怪物实例已变化或卸载，请重新选择并反编译".into());
    }
    let actor = target.pool as usize + usize::from(target.slot) * monster::STRIDE;
    let descriptor = unsafe { get::<u32>(actor + DESCRIPTOR) };
    let session = unsafe { state.read::<usize>(0x1e7fff3c) };
    if session == 0 {
        return Err("当前任务尚未就绪".into());
    }
    let map = unsafe { get::<u32>(session + 0x34) };
    let root = std::path::Path::new("dat/monster-ai");
    match operation {
        AiOperation::ReplaceSpecies(_) => Err("种类修改必须通过任务重载处理".into()),
        AiOperation::Load => {
            let project = ai::dsl::Project::load(root, map, target.species)
                .map_err(|e| e.to_string())?
                .ok_or("没有此地图或物种的 AI 工程")?;
            Ok(AiDocument {
                descriptor,
                source: Some(project),
                message: "已读取工程；编辑后点击应用热替换。".into(),
            })
        }
        AiOperation::Inspect => inspect_document(
            descriptor,
            target.species,
            unsafe { get(actor + 2576) },
            map,
        ),
        AiOperation::Apply {
            descriptor: expected,
            mut source,
        } => {
            check_descriptor(descriptor, expected)?;
            source
                .check_target(map, target.species)
                .map_err(|e| e.to_string())?;
            source.complete(root).map_err(|e| e.to_string())?;
            let compiled = source.compile().map_err(|e| e.to_string())?;
            if compiled.program.species != target.species || compiled.program.base != Base::Native {
                return Err("物种编号必须与当前怪物一致，且必须保留 base native;".into());
            }
            let mut blocks = Blocks::default();
            let overlay = ai::bind::materialize(&compiled.program, descriptor, &Live, &mut blocks)
                .map_err(|e| e.to_string())?;
            let first = Live.word(overlay.state_table).map_err(|e| e.to_string())?;
            if first == 0 {
                return Err("状态 0 不可为空".into());
            }
            if editor.retained_bytes + blocks.bytes > MAX_RETAINED {
                return Err("本次会话的 AI 热替换存储已达到 32 MiB，请重新启动调试会话".into());
            }
            // Finish every fallible operation before publishing native pointers.
            if !editor
                .originals
                .iter()
                .any(|(original, _)| *original == target)
            {
                editor.originals.push((target, descriptor));
            }
            editor.retained_bytes += blocks.bytes;
            editor.retained.push(blocks);
            unsafe {
                publish(actor, overlay.descriptor, first);
            }
            Ok(AiDocument {
                descriptor: overlay.descriptor,
                source: Some(source),
                message: format!(
                    "已热替换此实例，从状态 0 重新开始。{} 条 native / 语义提示；未写入文件。",
                    compiled.warnings.len()
                ),
            })
        }
        AiOperation::Restore {
            descriptor: expected,
        } => {
            check_descriptor(descriptor, expected)?;
            let original = editor
                .originals
                .iter()
                .find(|(original, _)| *original == target)
                .map(|(_, descriptor)| *descriptor)
                .ok_or("此实例没有调试热替换记录")?;
            let table = Live.word(original).map_err(|e| e.to_string())?;
            let first = Live.word(table).map_err(|e| e.to_string())?;
            if first == 0 {
                return Err("原始状态 0 不可为空".into());
            }
            // Prepare the refreshed text before committing the restore.
            let mut document = inspect_document(original, target.species, 0, map)?;
            document.message = "已恢复首次热替换前的 AI，从状态 0 重新开始。".into();
            unsafe {
                publish(actor, original, first);
            }
            Ok(document)
        }
    }
}

fn inspect_document(
    descriptor: u32,
    species: u8,
    current: u8,
    map: u32,
) -> Result<AiDocument, String> {
    let document = ai::decompile::decompile(&Live, descriptor, species, current, Some(map))
        .map_err(|e| e.to_string())?;
    Ok(AiDocument {
        descriptor,
        source: Some(ai::dsl::Project::single(
            Some(map),
            species,
            document.source,
        )),
        message: if document.warnings.is_empty() {
            "已反编译可达状态、事件和子脚本；未导出项沿用原生。".into()
        } else {
            format!("部分脚本沿用原生：\n{}", document.warnings.join("\n"))
        },
    })
}

/// Primary records are 60 bytes (10AB2450). 10AAA420 writes the actor slot
/// into +52/+56; do not infer ownership from species or current area alone.
fn primary_spawn(bytes: &[u8], species: u8, slot: u16) -> Result<usize, String> {
    let word = |offset: usize| -> Result<u32, String> {
        bytes
            .get(offset..offset + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| "任务记录越界".into())
    };
    let section = word(24)? as usize;
    if section == 0 || section > bytes.len().saturating_sub(16) {
        return Err("任务缺少怪物段".into());
    }
    let mut offset = word(section + 12)? as usize;
    if offset == 0 || offset > bytes.len().saturating_sub(2) {
        return Err("任务缺少目标怪物记录".into());
    }
    let mut found = None;
    loop {
        let header = bytes.get(offset..offset + 2).ok_or("任务出生记录越界")?;
        let id = u16::from_le_bytes(header.try_into().unwrap());
        if id == 0 || id == u16::MAX {
            break;
        }
        let record = bytes.get(offset..offset + 60).ok_or("任务出生记录被截断")?;
        if id == u16::from(species)
            && u16::from(record[52]) == slot
            && u16::from(record[56]) == slot
            && found.replace(offset).is_some()
        {
            return Err("多个出生记录指向此实例，无法确定修改目标".into());
        }
        offset += 60;
    }
    found.ok_or_else(|| "此实例不是可修改的任务目标；动态召唤、机关及变身实例暂不支持".into())
}

pub(super) unsafe fn replace_species(
    state: &State,
    runtime: &mut super::Runtime,
    target: AiTarget,
    species: u8,
) -> Result<AiDocument, String> {
    if !unsafe { targets(state, runtime.ai.epoch) }.contains(&target) {
        return Err("怪物实例已变化或卸载，请重新选择".into());
    }
    if species == target.species {
        return Err("请选择不同的怪物种类".into());
    }
    if !runtime
        .catalog
        .monsters
        .iter()
        .any(|monster| monster.id == species)
    {
        return Err("客户端没有此怪物种类".into());
    }
    let size = state.session.snapshot().quest_size;
    if !(0x86..=0x8000).contains(&size) {
        return Err("任务缓冲区长度无效".into());
    }
    let buffer = unsafe { state.read::<u32>(0x1ed528f4) };
    let bytes = Live.bytes(buffer, size).map_err(|e| e.to_string())?;
    let offset = primary_spawn(&bytes, target.species, target.slot)?;
    // Validate the restart prerequisite before publishing the replacement.
    unsafe { super::equipment::refresh_resource_indices(state.model()) }?;
    unsafe {
        state
            .session
            .replace_monster(
                mhf_quest::SpawnOffset::from_byte_offset(offset as u32),
                target.species,
                species,
            )
            .map_err(|e| e.to_string())?;
        super::restart(state, runtime)?;
    }
    Ok(AiDocument {
        descriptor: 0,
        source: None,
        message: "已修改目标种类，正在重载任务；任务目标条件保持原样。".into(),
    })
}

fn check_descriptor(current: u32, expected: u32) -> Result<(), String> {
    if current != expected {
        Err("AI 基座已变化，请重新反编译后再应用".into())
    } else {
        Ok(())
    }
}

/// Mirrors actor-local restart cleanup in the verified 10860430, plus clears
/// saved continuations and re-arms the selector. No quest/model reload occurs.
unsafe fn publish(actor: usize, descriptor: u32, first: u32) {
    unsafe {
        put(actor + DESCRIPTOR, descriptor);
        for offset in [3288, 3228] {
            put(actor + offset, 0_u16);
        }
        for offset in [2622, 3250, 2576, 2580, 2659, 2623] {
            put(actor + offset, 0_u8);
        }
        for offset in [
            2552, 2556, 2588, 2596, 2604, 2608, 2616, 2632, 2636, 2640, 2644, 2648,
        ] {
            put(actor + offset, 0_u32);
        }
        put(actor + 2548, first);
        put(actor + 2652, first);
        for offset in [2845, 2844, 2840, 2841] {
            put(actor + offset, u8::MAX);
        }
        if get::<u8>(actor + 3) != 0x8e {
            put(actor + 2846, u16::MAX);
            put(actor + 3208, u16::MAX);
        }
        put(actor + 2628, 0_u32);
        put(actor + 2601, 1_u8);
    }
}

struct Live;

impl Memory for Live {
    fn bytes(&self, address: u32, length: usize) -> ai::Result<Vec<u8>> {
        if address == 0 || length == 0 || address.checked_add(length as u32).is_none() {
            return Err(ai::Error::new("invalid AI memory range"));
        }
        let mut bytes = vec![0; length];
        let mut read = 0;
        unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                address as *const _,
                bytes.as_mut_ptr().cast(),
                length,
                Some(&mut read),
            )
        }
        .map_err(|e| ai::Error::new(format!("AI memory {address:#010x}: {e}")))?;
        if read != length {
            return Err(ai::Error::new("short AI memory read"));
        }
        Ok(bytes)
    }
}

impl NativeMemory for Live {
    fn read(&self, address: u32, words: usize) -> ai::Result<Vec<u32>> {
        if !address.is_multiple_of(4) {
            return Err(ai::Error::new("unaligned AI table"));
        }
        Ok(self
            .bytes(address, words * 4)?
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(u32::from_le_bytes)
            .collect())
    }
}

#[derive(Default)]
struct Blocks {
    blocks: Vec<Box<[u32]>>,
    bytes: usize,
}

impl Arena for Blocks {
    fn allocate(&mut self, words: usize) -> ai::Result<u32> {
        let block = vec![0; words.max(1)].into_boxed_slice();
        let address = u32::try_from(block.as_ptr() as usize)
            .map_err(|_| ai::Error::new("AI allocation outside i686"))?;
        self.bytes += block.len() * 4;
        self.blocks.push(block);
        Ok(address)
    }
    fn write(&mut self, address: u32, words: &[u32]) -> ai::Result<()> {
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.as_ptr() as usize == address as usize)
            .ok_or_else(|| ai::Error::new("unknown AI allocation"))?;
        block.copy_from_slice(words);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_spawn_resolves_slot_not_species_or_area() {
        let mut bytes = vec![0; 256];
        bytes[24..28].copy_from_slice(&32u32.to_le_bytes());
        bytes[44..48].copy_from_slice(&64u32.to_le_bytes());
        for (offset, slot) in [(64, 3), (124, 7)] {
            bytes[offset..offset + 2].copy_from_slice(&163u16.to_le_bytes());
            bytes[offset + 52] = slot;
            bytes[offset + 56] = slot;
        }
        assert_eq!(primary_spawn(&bytes, 163, 7).unwrap(), 124);
        assert!(primary_spawn(&bytes, 163, 8).is_err());
        assert!(primary_spawn(&bytes, 6, 7).is_err());
        bytes[64 + 52] = 7;
        bytes[64 + 56] = 7;
        assert!(primary_spawn(&bytes, 163, 7).is_err());
        bytes[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(primary_spawn(&bytes, 163, 7).is_err());
    }

    #[test]
    fn lists_loaded_instances_across_areas_and_detects_slot_reuse() {
        let mut pool = vec![0u8; monster::STRIDE * monster::SLOTS];
        let base = pool.as_mut_ptr() as usize;
        unsafe {
            for (slot, area) in [(0, 10u16), (7, 99)] {
                let actor = base + slot * monster::STRIDE;
                put(actor, 1u8);
                put(actor + 3, 6u8);
                put(actor + 2040, area);
                put(actor + DESCRIPTOR, 0x123400u32);
                put(actor + 3448, slot as u32 + 1);
                put(actor + 2576, slot as u8 + 2);
                put(actor + 476, 12.5_f32);
            }
            let before = pool_targets(base, 1);
            assert_eq!(before.iter().map(|t| t.slot).collect::<Vec<_>>(), [0, 7]);
            let status = statuses(&before);
            assert_eq!(status[1].target, before[1]);
            assert_eq!(status[1].ai_state, 9);
            assert_eq!(status[1].frame, 12.5);
            put(base + 3448, 42u32);
            assert!(!pool_targets(base, 1).contains(&before[0]));
            put(base + 7 * monster::STRIDE, 0u8);
            assert_eq!(pool_targets(base, 1).len(), 1);
            assert!(!pool_targets(base, 2).contains(&before[0]));
        }
    }

    #[test]
    fn publishing_restarts_cursors_but_preserves_model_position_and_action() {
        let mut actor = vec![0x55u8; monster::STRIDE];
        let address = actor.as_mut_ptr() as usize;
        let before = actor.clone();
        unsafe {
            publish(address, 0x1000, 0x2000);
            assert_eq!(get::<u32>(address + DESCRIPTOR), 0x1000);
            assert_eq!(get::<u32>(address + 2548), 0x2000);
            assert_eq!(get::<u32>(address + 2652), 0x2000);
            assert_eq!(get::<u16>(address + 3288), 0);
            assert_eq!(get::<u32>(address + 2552), 0);
            assert_eq!(get::<u8>(address + 2601), 1);
        }
        assert_eq!(&actor[..2544], &before[..2544]);
        assert_eq!(&actor[3448..], &before[3448..]);
    }

    #[test]
    fn checked_memory_rejects_bad_addresses_and_retains_old_allocations() {
        assert!(Live.bytes(0, 4).is_err());
        assert!(Live.bytes(1, 4).is_err());
        let mut blocks = Blocks::default();
        let address = blocks.allocate(2).unwrap();
        blocks.write(address, &[7, 9]).unwrap();
        let mut editor = Editor::default();
        editor.retained.push(blocks);
        editor.observe(123);
        editor.invalidate();
        assert_eq!(Live.read(address, 2).unwrap(), [7, 9]);
        assert!(check_descriptor(7, 8).is_err());
    }
}
