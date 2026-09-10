use super::{BASE, SLOT, State, get, monster, put};
use crate::provider::{CombatSnapshot, MonsterHealth};
use std::{
    cell::Cell,
    collections::HashMap,
    mem::transmute,
    sync::{PoisonError, atomic::Ordering},
};

thread_local! {
    static HOSTILE_TARGET: Cell<usize> = const { Cell::new(0) };
}

#[derive(Default)]
pub(super) struct Diagnostics {
    checks: u32,
    hits: u32,
    health: HashMap<(usize, u32), i32>,
    last_damage: String,
}

pub(super) unsafe fn snapshot(state: &State) -> CombatSnapshot {
    unsafe {
        let pool = state.read::<usize>(monster::POOL);
        let scene = state.read::<usize>(0x1e7fff3c);
        if pool == 0 || scene == 0 {
            return CombatSnapshot::default();
        }
        let area = get::<u16>(scene + 20);
        let controlled = state.controlled_monster.load(Ordering::Relaxed);
        let hp: unsafe extern "fastcall" fn(i32, usize) -> i32 =
            transmute(state.address(0x10aa0d70));
        let mut health = Vec::new();
        let mut diagnostics = state.combat.lock().unwrap_or_else(PoisonError::into_inner);
        let mut current = HashMap::new();
        for slot in 0..monster::SLOTS {
            let actor = pool + slot * monster::STRIDE;
            if get::<u8>(actor) == 0 || get::<u16>(actor + 2040) != area {
                continue;
            }
            let species = get::<u8>(actor + 3);
            let value = hp(0, actor).max(0);
            let key = (actor, get::<u32>(actor + 3448));
            if let Some(previous) = diagnostics.health.get(&key).copied()
                && previous > value
            {
                let name = crate::provider::monsters::NAMES
                    .get(usize::from(species))
                    .unwrap_or(&"未知怪物");
                diagnostics.last_damage =
                    format!("{name} #{slot} 扣血 {}，剩余 HP {value}", previous - value);
            }
            current.insert(key, value);
            health.push(MonsterHealth {
                slot: slot as u16,
                species,
                hp: value,
                controlled: actor == controlled,
            });
        }
        diagnostics.health = current;
        health.sort_by_key(|monster| (!monster.controlled, monster.slot));
        CombatSnapshot {
            checks: diagnostics.checks,
            hits: diagnostics.hits,
            health,
            last_damage: diagnostics.last_damage.clone(),
        }
    }
}

/// Keep the hunter available to native enemy targeting, and hide its model only
/// while submitting the world draw. The map continues using its synced position.
pub(super) unsafe extern "C" fn render_world() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x00b7b570) };
        return unsafe { original() };
    };
    unsafe {
        let visibility = controlled(state)
            .and_then(|_| monster::hunter(state))
            .map(|player| {
                let visible = get::<u8>(player + 1);
                put(player + 1, 0_u8);
                (player, visible)
            });
        let original: unsafe extern "C" fn() -> i32 = transmute(state.render_world);
        let result = original();
        if let Some((player, visible)) = visibility {
            put(player + 1, visible);
        }
        result
    }
}

/// Native effect collision normally excludes monsters when flag 0x80 is set.
/// The controlled actor's attacks must reach enemies while retaining their native
/// shapes, damage tables, hit reactions and hit cooldowns.
pub(super) unsafe extern "C" fn collide_effects() {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x008b6bf0) };
        return unsafe { original() };
    };
    unsafe {
        if let Some(actor) = controlled(state) {
            let mut effect = state.read::<usize>(0x1ecb1660);
            while effect != 0 {
                if get::<u16>(effect) != 0
                    && get::<u8>(effect + 15) == 2
                    && effect_owner(state, effect) == Some(actor)
                {
                    put(effect + 144, get::<u16>(effect + 144) & !0x80);
                    put(effect + 245, 0_u8);
                }
                effect = get(effect + 20);
            }
        }
        let original: unsafe extern "C" fn() = transmute(state.collide_effects);
        original();
    }
}

unsafe fn controlled(state: &State) -> Option<usize> {
    let actor = state.controlled_monster.load(Ordering::Relaxed);
    (actor != 0 && unsafe { monster::active_scene(state) }).then_some(actor)
}

unsafe fn effect_owner(state: &State, effect: usize) -> Option<usize> {
    unsafe {
        if get::<u8>(effect + 12) == u8::MAX || get::<u8>(effect + 146) == 0 {
            return None;
        }
        let kind = get::<u16>(effect + 4);
        if kind == 70 || (kind == 62 && get::<u16>(effect + 144) & 0x4000 != 0) {
            return Some(get(effect + 172));
        }
        let pool = state.read::<usize>(monster::POOL);
        let slot = usize::from(get::<u8>(effect + 14));
        (pool != 0 && slot < monster::SLOTS).then_some(pool + slot * monster::STRIDE)
    }
}

// 108B7B60: EDI=effect, ESI=recipient, stack=source. Adapt this usercall to C
// without changing the caller's stack cleanup or its callee-saved registers.
#[unsafe(naked)]
pub(super) unsafe extern "C" fn hit_target() {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push dword ptr [ebp + 8]",
        "push esi",
        "push edi",
        "call {}",
        "add esp, 12",
        "pop ebp",
        "ret",
        sym hit_target_impl,
    );
}

unsafe extern "C" fn hit_target_impl(effect: usize, mut target: usize, source: usize) {
    let invocation = SLOT.enter();
    let mut hostile = false;
    let original = if let Some(state) = invocation.state() {
        unsafe {
            if let Some(actor) = controlled(state) {
                let player = monster::hunter(state);
                if source == actor {
                    if target == actor || player == Some(target) {
                        return;
                    }
                } else if player == Some(target) {
                    // Enemy AI attacks the hunter proxy. Resolve the actual hit
                    // against the controlled monster's body, HP and damage state.
                    target = actor;
                    let hit_id = get::<u16>(effect + 220);
                    if (0..32).any(|index| get::<u16>(actor + 1186 + index * 2) == hit_id) {
                        return;
                    }
                }
                hostile = target == actor || (source == actor && get::<u8>(target + 16) == 1);
            }
        }
        state.hit_target
    } else {
        BASE.load(Ordering::Relaxed) + 0x008b7b60
    };
    unsafe {
        let previous_target = HOSTILE_TARGET.replace(if hostile { target } else { 0 });
        let faction = get::<u8>(effect + 245);
        let hit_id = get::<u16>(effect + 220);
        let had_hit =
            hostile && (0..32).any(|index| get::<u16>(target + 1186 + index * 2) == hit_id);
        if hostile {
            // The monster-to-monster final damage path has its own faction check.
            put(effect + 245, 0_u8);
        }
        // Save the call address on the stack before EDI takes the effect pointer.
        core::arch::asm!(
            "push esi",
            "push edi",
            "mov esi, eax",
            "mov edi, ecx",
            "push edx",
            "call dword ptr [esp + 4]",
            "add esp, 4",
            "pop edi",
            "pop esi",
            in("edi") original,
            inlateout("eax") target => _,
            in("ecx") effect,
            in("edx") source,
            clobber_abi("C"),
        );
        HOSTILE_TARGET.set(previous_target);
        if hostile {
            put(effect + 245, faction);
            if let Some(state) = invocation.state() {
                let mut diagnostics = state.combat.lock().unwrap_or_else(PoisonError::into_inner);
                diagnostics.checks = diagnostics.checks.saturating_add(1);
                if !had_hit && (0..32).any(|index| get::<u16>(target + 1186 + index * 2) == hit_id)
                {
                    diagnostics.hits = diagnostics.hits.saturating_add(1);
                }
            }
        }
    }
}

// 10846CA0: EAX=actor, CX=mode, stack=part offset, returns the native hurtbox list.
#[unsafe(naked)]
pub(super) unsafe extern "C" fn hurt_shapes() {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push dword ptr [ebp + 8]",
        "push ecx",
        "push eax",
        "call {}",
        "add esp, 12",
        "pop ebp",
        "ret",
        sym hurt_shapes_impl,
    );
}

unsafe extern "C" fn hurt_shapes_impl(actor: usize, mode: u32, part: usize) -> usize {
    let invocation = SLOT.enter();
    let address = invocation.state().map_or_else(
        || BASE.load(Ordering::Relaxed) + 0x00846ca0,
        |state| state.hurt_shapes,
    );
    unsafe {
        let original: usize;
        core::arch::asm!(
            "push edi",
            "call edx",
            "add esp, 4",
            in("edi") part,
            in("edx") address,
            inlateout("eax") actor => original,
            in("ecx") mode,
            clobber_abi("C"),
        );
        let Some(state) = invocation.state() else {
            return original;
        };
        if original == 0 || part != 0 || HOSTILE_TARGET.get() != actor {
            return original;
        }
        let mut records = Vec::new();
        for index in 0..256 {
            let entry = original + index * 40;
            if get::<u16>(entry) == u16::MAX {
                let mut terminator = [0_u8; 40];
                terminator[..2].copy_from_slice(&u16::MAX.to_le_bytes());
                records.push(terminator);
                // Descriptors may reside in read-only shared species data. Keep
                // stable copies until hook teardown, and refresh their geometry.
                let mut copies = state
                    .hurtbox_copies
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                return copies
                    .entry((original, records.len()))
                    .and_modify(|copy| copy.copy_from_slice(&records))
                    .or_insert_with(|| records.into_boxed_slice())
                    .as_ptr() as usize;
            }
            let mut record = get::<[u8; 40]>(entry);
            // 108C5910 requires +8 bit 1 for monster attack types 5/14/38.
            // Keep conditional, disabled and special-only hurtboxes unchanged.
            if get::<u16>(entry) != 125 && record[8] & 4 == 0 && record[9] & 0x16 == 0 {
                record[8] |= 2;
            }
            records.push(record);
        }
        original
    }
}
