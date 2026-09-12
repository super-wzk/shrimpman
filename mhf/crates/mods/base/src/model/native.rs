//! Native model operations for the supported ZZ HD client.
//!
//! Every memory operation must run while the verified game DLL and its resources
//! remain loaded. Model mutations must run on the game's task thread, after its
//! local hunter is initialized and before native update/render resumes.

use super::{
    Appearance, AppearanceChange, AppearanceOptions, Equipment, EquipmentCatalog, Face, Transmogs,
};
use std::{mem::transmute, ptr};
use windows::Win32::Globalization::{MB_ERR_INVALID_CHARS, MultiByteToWideChar};

#[derive(Clone, Copy, Debug)]
pub struct Client {
    base: usize,
}

impl Client {
    /// # Safety
    /// `base` must be a live supported `mhfo-hd.dll` image. The caller must keep
    /// that module loaded throughout all uses and verify its native interfaces.
    pub const unsafe fn new(base: usize) -> Self {
        Self { base }
    }

    pub fn address(self, preferred_va: usize) -> usize {
        self.base + preferred_va - 0x1000_0000
    }

    /// # Safety
    /// The relocated address must contain an initialized readable `T`; no game
    /// thread may mutate it concurrently with this read.
    pub unsafe fn read<T: Copy>(self, preferred_va: usize) -> T {
        unsafe { get(self.address(preferred_va)) }
    }

    /// # Safety
    /// The module's mapped image must include every checked function range.
    pub unsafe fn validate(self) -> Result<(), String> {
        for &(rva, expected) in SIGNATURES {
            if unsafe { std::slice::from_raw_parts((self.base + rva) as *const u8, expected.len()) }
                != expected
            {
                return Err(format!("不支持此游戏 DLL 的模型接口：RVA {rva:#x}"));
            }
        }
        Ok(())
    }
}

unsafe fn get<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_unaligned(address as *const T) }
}

unsafe fn put<T>(address: usize, value: T) {
    unsafe { ptr::write_unaligned(address as *mut T, value) }
}

/// # Safety
/// The client must be live and its scene pointer readable on the game thread.
pub unsafe fn hunter(client: Client) -> Option<usize> {
    unsafe {
        let scene = client.read::<usize>(0x1e7fff3c);
        if scene == 0 {
            return None;
        }
        let index = usize::from(get::<u8>(scene + 9208));
        (index < 4).then(|| client.address(0x1dc6b750) + index * 4176)
    }
}

/// RVA byte prefixes shared by consumers before installing hooks.
pub const SIGNATURES: &[(usize, &[u8])] = &[
    (
        0x00ba7160,
        &[0x0f, 0xb7, 0xd0, 0x03, 0xd2, 0xf6, 0x84, 0xd1],
    ),
    (
        0x00b9f080,
        &[0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d, 0x0c, 0x56],
    ),
    (
        0x00a89cd0,
        &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x81, 0xec],
    ),
    (
        0x001c0780,
        &[0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d, 0x08, 0x56],
    ),
    (
        0x00b37680,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x20, 0x53, 0x56],
    ),
    (
        0x00b9f820,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x53, 0x8b],
    ),
    (
        0x008f9960,
        &[0x56, 0x8d, 0xb7, 0x00, 0x04, 0x00, 0x00, 0x6a],
    ),
    (
        0x008fb7c0,
        &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x88, 0x00, 0x00],
    ),
    (
        0x008fb1f0,
        &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x83, 0xec, 0x1c, 0x53],
    ),
    (
        0x008fca00,
        &[0x55, 0x8b, 0xec, 0x83, 0x7d, 0x08, 0x00, 0x57],
    ),
    (
        0x0089f8c0,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x53, 0x56],
    ),
    (
        0x00a92d70,
        &[0x55, 0x8b, 0xec, 0x51, 0x85, 0xf6, 0x74, 0x3b],
    ),
    // Skip the relocated absolute address in the first MOVSS instruction.
    (
        0x008ec098,
        &[0x0f, 0x57, 0xc0, 0x53, 0x56, 0x57, 0x8b, 0xf8],
    ),
    (
        0x00bba300,
        &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08, 0x56, 0x57],
    ),
];

unsafe fn text(pointer: usize) -> Option<String> {
    if pointer == 0 {
        return None;
    }
    // All catalog pointers come from the supported, relocated DAT resource.
    let mut length = 0;
    while length < 1024 && unsafe { get::<u8>(pointer + length) } != 0 {
        length += 1;
    }
    if length == 0 || length == 1024 {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer as *const u8, length) };
    decode_catalog_text(bytes)
}

fn decode_catalog_text(bytes: &[u8]) -> Option<String> {
    // Catalog strings in the supported Japanese DAT use CP932.
    let length = unsafe { MultiByteToWideChar(932, MB_ERR_INVALID_CHARS, bytes, None) };
    if length <= 0 {
        return None;
    }
    let mut utf16 = vec![0; length as usize];
    let written =
        unsafe { MultiByteToWideChar(932, MB_ERR_INVALID_CHARS, bytes, Some(&mut utf16)) };
    if written != length {
        return None;
    }
    String::from_utf16(&utf16).ok()
}

/// Read only the equipment and appearance directories from the relocated DAT.
///
/// # Safety
/// The client and supported DAT tables must be live and stable on the game thread.
pub unsafe fn catalog(client: Client) -> EquipmentCatalog {
    let mut catalog = EquipmentCatalog::default();
    unsafe {
        let dat = client.read::<usize>(0x1e77dcc4);
        if dat == 0 {
            return catalog;
        }
        catalog.appearances = appearance_options(dat);
        // Name-table extents match shared/resource/resources/layout.json for this ZZ DAT.
        // Native 10A9BDF0 resolves these spec tables and record strides.
        // 10BAD8B0 reads weapon model +0; 108F9D00 reads armor male/female +0/+2.
        for (kind, root, count, specs, stride, class_offset) in [
            (6, 136, 17568, 124, 52, Some(3)),
            (7, 132, 4223, 128, 60, Some(4)),
            (2, 100, 14594, 80, 72, None),
            (3, 104, 13462, 84, 72, None),
            (4, 108, 13452, 88, 72, None),
            (5, 112, 13708, 92, 72, None),
            (0, 116, 13514, 96, 72, None),
        ] {
            let names = get::<usize>(dat + root);
            let specs = get::<usize>(dat + specs);
            if names == 0 || specs == 0 {
                continue;
            }
            for id in 0..count {
                let spec = specs + id * stride;
                let weapon = class_offset.map(|offset| get::<u8>(spec + offset));
                if weapon.is_some_and(|weapon| weapon >= 14) {
                    continue;
                }
                let Some(name) = text(get::<usize>(names + id * 4)) else {
                    continue;
                };
                catalog.equipment.push(Equipment {
                    kind,
                    id: id as u16,
                    model_ids: if weapon.is_some() {
                        [get::<u16>(spec); 2]
                    } else {
                        get::<[u16; 2]>(spec)
                    },
                    weapon,
                    name,
                });
            }
        }
    }
    catalog
}

unsafe fn local_hunter(client: Client) -> Result<(usize, usize), String> {
    unsafe {
        let scene = client.read::<usize>(0x1e7fff3c);
        let player = hunter(client).ok_or("猎人尚未初始化")?;
        // Other hunters can share motion allocations; only reload the offline
        // session's single hunter in slot 0.
        if scene == 0 || get::<u8>(scene + 9208) != 0 || get::<u8>(scene + 9210) != 1 {
            return Err("热替换仅支持本地单猎人调试任务".into());
        }
        let save = client.read::<usize>(0x11a3ee2c);
        if save == 0 || get::<usize>(player + 3368) == 0 {
            return Err("猎人装备尚未初始化".into());
        }
        Ok((player, save))
    }
}

/// # Safety
/// `save` must point to the live supported save data with no concurrent writes.
pub unsafe fn appearance(save: usize) -> Appearance {
    unsafe {
        Appearance {
            female: get::<u8>(save + 1) != 0,
            face: get(save + 2),
            hair: get(save + 3),
        }
    }
}

/// # Safety
/// A nonzero `dat` must be the supported relocated DAT, including all referenced
/// name/model tables, and remain readable for this call.
pub unsafe fn appearance_options(dat: usize) -> [AppearanceOptions; 2] {
    unsafe {
        if dat == 0 {
            return Default::default();
        }
        let header = get::<usize>(dat + 4 * 4);
        let model_counts = get::<usize>(dat + 57 * 4);
        let face_records = get::<usize>(dat + 200 * 4);
        if header == 0 || model_counts == 0 || face_records == 0 {
            return Default::default();
        }
        // 10B9F080 resolves a byte-sized face index through six-byte records.
        let face_count = usize::from(get::<u16>(header + 220)).min(256);
        std::array::from_fn(|gender| {
            // 10A6B470 uses this model count and a one-based face model ID;
            // 108FB1F0 loads its zero-based counterpart (record.model - 1).
            let face_models = get::<u16>(model_counts + 2 * (7 * gender + 1));
            let faces = (0..face_count)
                .filter_map(|face| {
                    let model = get::<u16>(face_records + 6 * face);
                    (model != 0 && model <= face_models).then(|| Face {
                        id: face as u8,
                        model_id: model - 1,
                    })
                })
                .collect();
            // 10836340's hairstyle selector offers 0..=150 except 17..=26.
            // 108F9D00 additionally bounds the head/hair ID with slot 2's
            // gender-specific model count before selecting its resource file.
            let hair_models = get::<u16>(model_counts + 2 * (7 * gender + 2));
            let hair = (0..=150_u8)
                .filter(|&hair| !(17..=26).contains(&hair) && u16::from(hair) < hair_models)
                .collect();
            AppearanceOptions { faces, hair }
        })
    }
}

/// # Safety
/// Run on the initialized local hunter's task thread with the live, verified
/// client. Options and transmogs must belong to its current DAT; an optional
/// moveset must be a native weapon class in `0..14`.
pub unsafe fn change_appearance(
    client: Client,
    moveset: Option<u8>,
    transmogs: &Transmogs,
    options: &[AppearanceOptions; 2],
    change: AppearanceChange,
) -> Result<(), String> {
    unsafe {
        let (player, save) = local_hunter(client)?;
        let current = appearance(save);
        let next = current.changed(change, options)?;
        if next == current {
            return Ok(());
        }
        reset_action(client, player);
        // 10B9F080 copies these fields, resolving the face directory index into
        // player+946 and the hairstyle into player+910. Keep the save in sync
        // so later equipment changes and area loads retain the new appearance.
        put(save + 1, u8::from(next.female));
        put(save + 2, next.face);
        put(save + 3, next.hair);
        refresh(client, moveset, transmogs, player, save, true);
    }
    Ok(())
}

/// # Safety
/// Run on the initialized local hunter's task thread with the live, verified
/// client. `kind`/`id` must come from its equipment catalogue, transmogs must have
/// been validated against that catalogue, and a moveset must be in `0..14`.
pub unsafe fn equip(
    client: Client,
    moveset: Option<u8>,
    transmogs: &Transmogs,
    kind: u8,
    id: u16,
) -> Result<(), String> {
    unsafe {
        let (player, save) = local_hunter(client)?;
        let add: unsafe extern "C" fn(usize, u8, u16, u16) -> i16 =
            transmute(client.address(0x10ba6b10));
        let capacity: unsafe extern "C" fn(usize) -> i16 = transmute(client.address(0x10ba9ad0));
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

        reset_action(client, player);

        let equipped: u32;
        std::arch::asm!(
            "call edx",
            in("edx") client.address(0x10ba7160),
            inlateout("eax") index as u32 => equipped,
            in("ecx") save,
            clobber_abi("C"),
        );
        if equipped == 0 {
            return Err("无法装备所选装备".into());
        }

        refresh(client, moveset, transmogs, player, save, false);
    }
    Ok(())
}

/// # Safety
/// `player` must be a writable native hunter on the task thread. Every nonzero
/// override ID must already have been validated against the supported DAT.
pub unsafe fn apply_transmogs(player: usize, transmogs: &Transmogs) {
    unsafe {
        // 108F9D00 resolves these equipment IDs through each armor DAT table.
        // They are separate from the equipped records at player+928 and from
        // the weapon's model/class. Slot 1 belongs to the face, not armor.
        for kind in [0, 2, 3, 4, 5] {
            put(player + 4012 + 2 * kind, transmogs.armor[kind]);
        }
    }
}

unsafe fn validate_transmogs(
    dat: usize,
    player: usize,
    transmogs: &Transmogs,
) -> Result<(), String> {
    if dat == 0 {
        return Err("防具目录尚未初始化".into());
    }
    unsafe {
        let female = get::<u8>(player + 17) != 0;
        let gender = if female { 2 } else { 1 };
        for (kind, table) in [(0, 24), (2, 20), (3, 21), (4, 22), (5, 23)] {
            let id = transmogs.armor[kind];
            if id == 0 || get::<u16>(player + 4012 + 2 * kind) == id {
                continue;
            }
            let equipped = get::<u16>(player + 930 + 16 * kind);
            // 108F9D00's head branch uses hair directly when no helmet is worn.
            if kind == 2 && equipped == 0 {
                return Err("请先装备头部防具，再设置头部幻化".into());
            }
            let specs = get::<usize>(dat + 4 * table);
            if specs == 0 {
                return Err("防具目录尚未初始化".into());
            }
            // Native resolution checks the worn armor's gender before using
            // its override; the helmet loader also checks the override itself.
            if get::<u8>(specs + 72 * usize::from(equipped) + 4) & gender == 0 {
                return Err("当前部位装备不支持猎人性别，请先更换防具".into());
            }
            if get::<u8>(specs + 72 * usize::from(id) + 4) & gender == 0 {
                return Err("所选幻化防具不支持当前猎人性别".into());
            }
        }
    }
    Ok(())
}

/// # Safety
/// Run on the initialized local hunter's task thread with the live, verified
/// client. Nonzero IDs must first pass `Transmogs::changed` for its catalogue;
/// this function additionally checks native gender/model compatibility.
pub unsafe fn change_transmog(client: Client, transmogs: &Transmogs) -> Result<(), String> {
    unsafe {
        let (player, _) = local_hunter(client)?;
        validate_transmogs(client.read(0x1e77dcc4), player, transmogs)?;
        if [0, 2, 3, 4, 5]
            .into_iter()
            .all(|kind| get::<u16>(player + 4012 + 2 * kind) == transmogs.armor[kind])
        {
            return Ok(());
        }
        reset_action(client, player);
        for kind in [0, 2, 3, 4, 5] {
            if get::<u16>(player + 4012 + 2 * kind) != transmogs.armor[kind] {
                // Different armor IDs can share a model but use different
                // attached effects. Force that part's native release/load.
                put(player + 3156 + 2 * kind, -1_i16);
            }
        }
        apply_transmogs(player, transmogs);
        // Keep weapon and motion allocations intact. Armor loading also rebuilds
        // the selected parts' cosmetic effects through 10BAFF00/10BB0200.
        let release: unsafe extern "thiscall" fn(usize) -> i32 =
            transmute(client.address(0x108fb7c0));
        let load: unsafe extern "C" fn(usize) = transmute(client.address(0x108fb1f0));
        release(player);
        load(player);
        // The chest owns animation buffers. Bind them before the next native
        // update, retaining the current weapon class and its loaded moveset.
        bind_animations(
            client.address(0x10a92d70),
            client.address(0x108ec090),
            client.address(0x10bba300),
            player,
        );
        reset_action(client, player);
    }
    Ok(())
}

unsafe fn reset_action(client: Client, player: usize) {
    unsafe {
        // End the old action while its weapon class and resources still agree.
        let change: unsafe extern "C" fn(usize, i16, i16, i16, u8) -> i16 =
            transmute(client.address(0x10a80a00));
        change(player, 0, 0, 2, 1);
        put(player + 18, 0_u8);
    }
}

unsafe fn refresh(
    client: Client,
    moveset: Option<u8>,
    transmogs: &Transmogs,
    player: usize,
    save: usize,
    reload_appearance: bool,
) {
    unsafe {
        // 10B9F080 also rebuilds the save's equipped records. 10B9FE90 would
        // overwrite the live item pouch, so refresh its equipment fields only.
        let copy_equipment: unsafe extern "C" fn(usize, usize) =
            transmute(client.address(0x10b9f080));
        let copy_armor_properties: unsafe extern "C" fn(usize) =
            transmute(client.address(0x10b9f820));
        let update_skills: unsafe extern "C" fn(usize) = transmute(client.address(0x10a89cd0));
        let cache_skills: unsafe extern "C" fn(usize) = transmute(client.address(0x101c0780));
        copy_equipment(player, save);
        // Copying equipment restores the save's native cosmetic fields, so
        // reapply this session's selections before any model resolution.
        apply_transmogs(player, transmogs);
        // EAX=player: refresh the equipped weapon's secret-book style.
        std::arch::asm!(
            "call edx",
            in("edx") client.address(0x10b37680),
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

        if reload_appearance {
            // 108FA150 compares only numeric model IDs, although 108FB1F0 loads
            // gender-specific files and face-dependent skin variants. Invalidate
            // all six body/face/head IDs even when their numbers are unchanged.
            // Retain resource handles/counts for 108FB7C0 to release them safely.
            for part in 0..6 {
                put(player + 3156 + 2 * part, -1_i16);
            }
        }

        // Diff the new equipment against the loaded model IDs, then synchronously
        // release/load just those models. Do not call 106A7FB0: it also frees
        // player+1720, which 10A5E9F8 dereferences on the very next hunter update.
        reload_models(
            client.address(0x108f9960),
            client.address(0x108fb7c0),
            client.address(0x108fca00),
            player,
        );

        // 108FD1D0 replaces motion allocations. Finish rebinding in this dispatch
        // before native update/render can observe an old animation or blend.
        reload_moveset(client.address(0x1089f8c0), u32::from(weapon));
        bind_animations(
            client.address(0x10a92d70),
            client.address(0x108ec090),
            client.address(0x10bba300),
            player,
        );
        reset_action(client, player);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transmogs_only_change_native_armor_overrides_and_can_restore_equipment() {
        let mut player = [0xa5_u8; 4176];
        let pointer = player.as_mut_ptr() as usize;
        let transmogs = Transmogs {
            armor: [11, 999, 22, 33, 44, 55],
        };
        let mut expected = player;
        for kind in [0, 2, 3, 4, 5] {
            let offset = 4012 + 2 * kind;
            expected[offset..offset + 2].copy_from_slice(&transmogs.armor[kind].to_le_bytes());
        }
        unsafe { apply_transmogs(pointer, &transmogs) };
        // Compare the whole hunter: equipped gear, weapon class, face, skills,
        // action state, animation pointers and resource handles stay untouched.
        assert_eq!(player, expected);

        for kind in [0, 2, 3, 4, 5] {
            expected[4012 + 2 * kind..4014 + 2 * kind].fill(0);
        }
        unsafe { apply_transmogs(pointer, &Transmogs::default()) };
        assert_eq!(player, expected);
    }

    #[test]
    fn transmog_validation_rejects_unequipped_helmets_and_unsupported_gender() {
        let mut player = [0_u8; 4176];
        let pointer = player.as_mut_ptr() as usize;
        let mut dat = [0_usize; 25];
        let mut specs = [[0_u8; 72]; 3];
        specs[1][4] = 3;
        specs[2][4] = 1;
        dat[20] = specs.as_ptr() as usize;
        let dat = dat.as_ptr() as usize;
        let mut transmogs = Transmogs::default();
        transmogs.armor[2] = 2;
        unsafe {
            assert!(validate_transmogs(dat, pointer, &transmogs).is_err());
            put(pointer + 962, 1_u16);
            assert!(validate_transmogs(dat, pointer, &transmogs).is_ok());
            put(pointer + 17, 1_u8);
            assert!(validate_transmogs(dat, pointer, &transmogs).is_err());
            put(specs.as_mut_ptr() as usize + 2 * 72 + 4, 3_u8);
            assert!(validate_transmogs(dat, pointer, &transmogs).is_ok());
            put(specs.as_mut_ptr() as usize + 72 + 4, 1_u8);
            assert!(validate_transmogs(dat, pointer, &transmogs).is_err());
            assert!(validate_transmogs(dat, pointer, &Transmogs::default()).is_ok());
        }
    }

    #[test]
    fn appearance_catalog_resolves_face_records_and_native_hair_ranges_per_gender() {
        let mut dat = [0_usize; 201];
        let mut counts = [0_u16; 111];
        let mut models = [0_u16; 14];
        let faces = [[0_u16; 3], [1, 0, 0], [2, 0, 0], [3, 0, 0], [500, 0, 0]];
        counts[110] = faces.len() as u16;
        models[1] = 2;
        models[8] = 3;
        models[2] = 151;
        models[9] = 30;
        dat[4] = counts.as_ptr() as usize;
        dat[57] = models.as_ptr() as usize;
        dat[200] = faces.as_ptr() as usize;

        let options = unsafe { appearance_options(dat.as_ptr() as usize) };
        assert_eq!(
            options[0].faces,
            [Face { id: 1, model_id: 0 }, Face { id: 2, model_id: 1 }]
        );
        assert_eq!(
            options[1].faces,
            [
                Face { id: 1, model_id: 0 },
                Face { id: 2, model_id: 1 },
                Face { id: 3, model_id: 2 },
            ]
        );
        assert_eq!(options[0].hair.len(), 141);
        assert_eq!(options[0].hair.first(), Some(&0));
        assert_eq!(options[0].hair.last(), Some(&150));
        assert_eq!(options[1].hair.len(), 20);
        assert_eq!(options[1].hair.last(), Some(&29));
        for options in options {
            assert!((17..=26).all(|hair| !options.hair.contains(&hair)));
        }
    }
}

#[cfg(test)]
mod text_tests {
    use super::decode_catalog_text;

    #[test]
    fn catalog_preserves_ascii_names() {
        assert_eq!(decode_catalog_text(b"Iron Sword").unwrap(), "Iron Sword");
    }

    #[test]
    fn catalog_decodes_original_cp932() {
        assert_eq!(
            decode_catalog_text(b"\x83\x65\x83\x58\x83\x67").unwrap(),
            "テスト"
        );
        assert!(decode_catalog_text(b"\x83").is_none());
    }
}
