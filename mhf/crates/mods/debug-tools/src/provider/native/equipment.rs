//! Refresh the local hunter through the native equipment loaders on the task thread.

use super::{State, get, monster, put};
use std::mem::transmute;

pub(super) unsafe fn equip(
    state: &State,
    moveset: Option<u8>,
    kind: u8,
    id: u16,
) -> Result<(), String> {
    unsafe {
        let scene = state.read::<usize>(0x1e7fff3c);
        let player = monster::hunter(state).ok_or("猎人尚未初始化")?;
        // Other hunters can share motion allocations; only reload the offline
        // provider's single hunter in slot 0.
        if scene == 0 || get::<u8>(scene + 9208) != 0 || get::<u8>(scene + 9210) != 1 {
            return Err("装备热替换仅支持本地单猎人调试任务".into());
        }
        let save = state.read::<usize>(0x11a3ee2c);
        if save == 0 || get::<usize>(player + 3368) == 0 {
            return Err("猎人装备尚未初始化".into());
        }
        let add: unsafe extern "C" fn(usize, u8, u16, u16) -> i16 =
            transmute(state.address(0x10ba6b10));
        let capacity: unsafe extern "C" fn(usize) -> i16 = transmute(state.address(0x10ba9ad0));
        let count = i32::from(capacity(save)) * 100;
        if !(1..=8000).contains(&count) {
            return Err("装备箱状态无效".into());
        }
        let existing = (0..count as usize).find(|index| {
            let item = save + 212 + index * 16;
            get::<u8>(item) & 1 != 0 && get::<u8>(item + 1) == kind && get::<u16>(item + 2) == id
        });
        let index = existing.map_or_else(|| add(save, kind, id, 0), |index| index as i16);
        if index < 0 {
            return Err("临时装备箱已满".into());
        }

        // End the old action while its weapon class and resources still agree.
        let change_action: unsafe extern "C" fn(usize, i16, i16, i16, u8) -> i16 =
            transmute(state.address(0x10a80a00));
        change_action(player, 0, 0, 2, 1);
        put(player + 18, 0_u8);

        let equipped: u32;
        std::arch::asm!(
            "call edx",
            in("edx") state.address(0x10ba7160),
            inlateout("eax") index as u32 => equipped,
            in("ecx") save,
            clobber_abi("C"),
        );
        if equipped == 0 {
            return Err("无法装备所选装备".into());
        }

        // 10B9F080 also rebuilds the save's equipped records. 10B9FE90 would
        // overwrite the live item pouch, so refresh its equipment fields only.
        let copy_equipment: unsafe extern "C" fn(usize, usize) =
            transmute(state.address(0x10b9f080));
        let copy_armor_properties: unsafe extern "C" fn(usize) =
            transmute(state.address(0x10b9f820));
        let update_skills: unsafe extern "C" fn(usize) = transmute(state.address(0x10a89cd0));
        let cache_skills: unsafe extern "C" fn(usize) = transmute(state.address(0x101c0780));
        copy_equipment(player, save);
        // EAX=player: refresh the equipped weapon's secret-book style.
        std::arch::asm!(
            "call edx",
            in("edx") state.address(0x10b37680),
            inlateout("eax") player => _,
            clobber_abi("C"),
        );
        copy_armor_properties(player);
        update_skills(player);
        cache_skills(player);

        // Match the quest constructor: moveset overrides also select the weapon
        // model's animation buffers; the actual model ID still comes from gear.
        let weapon = moveset.unwrap_or_else(|| get(player + 3));
        put(player + 3, weapon);

        // Diff the new equipment against the loaded model IDs, then synchronously
        // release/load just those models. Do not call 106A7FB0: it also frees
        // player+1720, which 10A5E9F8 dereferences on the very next hunter update.
        reload_models(
            state.address(0x108f9960),
            state.address(0x108fb7c0),
            state.address(0x108fca00),
            player,
        );

        // 108FD1D0 replaces motion allocations. Finish rebinding in this dispatch
        // before native update/render can observe an old animation or blend.
        reload_moveset(state.address(0x1089f8c0), u32::from(weapon));
        bind_animations(
            state.address(0x10a92d70),
            state.address(0x108ec090),
            state.address(0x10bba300),
            player,
        );
        change_action(player, 0, 0, 2, 1);
    }
    Ok(())
}

/// Release weapon (EDI) and armor (ECX), then load models (ESI, stack=0).
#[unsafe(naked)]
unsafe extern "C" fn reload_models(
    _release_weapon: usize,
    _release_armor: usize,
    _load: usize,
    _player: usize,
) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push esi",
        "push edi",
        "mov esi, [ebp + 20]",
        "mov edi, esi",
        "call dword ptr [ebp + 8]",
        "mov ecx, esi",
        "call dword ptr [ebp + 12]",
        "push 0",
        "call dword ptr [ebp + 16]",
        "add esp, 4",
        "pop edi",
        "pop esi",
        "pop ebp",
        "ret",
    );
}

/// 1089F8C0 takes EAX=slot 0 and the weapon class on the caller-clean stack.
#[unsafe(naked)]
unsafe extern "C" fn reload_moveset(_target: usize, _weapon: u32) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "xor eax, eax",
        "push dword ptr [ebp + 12]",
        "call dword ptr [ebp + 8]",
        "add esp, 4",
        "pop ebp",
        "ret",
    );
}

/// Follow 106A7D40's local animation/bone/effect sequence without its town state.
/// 10A92D70 takes ESI=player, EDI=channel and stack=animation; the other two use EAX.
#[unsafe(naked)]
unsafe extern "C" fn bind_animations(
    _animate: usize,
    _skeleton: usize,
    _effects: usize,
    _player: usize,
) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push esi",
        "push edi",
        "mov esi, [ebp + 20]",
        "xor edi, edi",
        "push 1",
        "call dword ptr [ebp + 8]",
        "add esp, 4",
        "mov edi, 1",
        "push 101",
        "call dword ptr [ebp + 8]",
        "add esp, 4",
        "mov eax, esi",
        "call dword ptr [ebp + 12]",
        "mov eax, esi",
        "call dword ptr [ebp + 16]",
        "pop edi",
        "pop esi",
        "pop ebp",
        "ret",
    );
}
