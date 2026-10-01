//! Register adapters for the verified native entrypoints. The external side is
//! cdecl; the game uses EAX/ECX/EDI/ESI parameters in several internal functions.

#[repr(C)]
pub(super) struct Registers {
    pub edi: u32,
    pub esi: u32,
    pub ebp: u32,
    pub esp: u32,
    pub ebx: u32,
    pub edx: u32,
    pub ecx: u32,
    pub eax: u32,
    pub flags: u32,
}

impl Registers {
    pub unsafe fn argument(&self, index: usize) -> u32 {
        // PUSHFD precedes PUSHAD: saved ESP -> flags, return address, arguments.
        unsafe { std::ptr::read_unaligned((self.esp as usize + 8 + index * 4) as *const u32) }
    }
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn load_detour() {
    core::arch::naked_asm!(
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        dispatch = sym super::load,
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn build_detour() {
    core::arch::naked_asm!(
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        dispatch = sym super::build,
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn weapon_texture_release_detour() {
    core::arch::naked_asm!(
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        dispatch = sym super::weapon_textures::release,
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn weapon_texture_constructor_detour() {
    core::arch::naked_asm!(
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        dispatch = sym super::weapon_textures::construct,
    );
}

/// ECX/EDX carry FSKL/TXB, while the caller owns the seven stack arguments.
#[unsafe(naked)]
pub(super) unsafe extern "C" fn build_resource_original(
    _target: usize,
    _skeleton: usize,
    _textures: usize,
    _resource: usize,
    _root: usize,
    _kind: u32,
    _texture_base: u32,
    _palette: u32,
    _flags: u32,
    _skinning: u32,
) -> usize {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "mov ecx, [ebp + 12]",
        "mov edx, [ebp + 16]",
        "push dword ptr [ebp + 44]",
        "push dword ptr [ebp + 40]",
        "push dword ptr [ebp + 36]",
        "push dword ptr [ebp + 32]",
        "push dword ptr [ebp + 28]",
        "push dword ptr [ebp + 24]",
        "push dword ptr [ebp + 20]",
        "call dword ptr [ebp + 8]",
        "add esp, 28",
        "pop ebp",
        "ret",
    );
}

/// The hook replaces a CALL instruction, so recreate its return address first.
#[unsafe(naked)]
pub(super) unsafe extern "C" fn equipment_cache_load_detour() {
    core::arch::naked_asm!(
        "push dword ptr [{resume}]",
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        resume = sym super::equipment_cache::SYNC_RETURN,
        dispatch = sym super::equipment_cache::load,
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn equipment_part_load_detour() {
    core::arch::naked_asm!(
        "push dword ptr [{resume}]",
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        resume = sym super::equipment_cache::SYNC_PART_RETURN,
        dispatch = sym super::equipment_cache::load,
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn stage_hd_load_detour() {
    core::arch::naked_asm!(
        "push dword ptr [{resume}]",
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        resume = sym super::stage_cache::HD_RETURN,
        dispatch = sym super::stage_cache::load,
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn stage_load_detour() {
    core::arch::naked_asm!(
        "push dword ptr [{resume}]",
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        resume = sym super::stage_cache::SD_RETURN,
        dispatch = sym super::stage_cache::load,
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn stage_decode_detour() {
    core::arch::naked_asm!(
        "pushfd", "pushad", "push esp", "call {dispatch}", "add esp, 4",
        "popad", "popfd", "ret",
        dispatch = sym super::stage_cache::decode,
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn decode_stage_pair(
    _target: usize,
    _fmod: usize,
    _fskl: usize,
    _source: usize,
) -> u32 {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push edi",
        "push esi",
        "mov eax, [ebp + 12]",
        "mov edi, [ebp + 16]",
        "mov esi, [ebp + 20]",
        "call dword ptr [ebp + 8]",
        "pop esi",
        "pop edi",
        "pop ebp",
        "ret",
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn read_equipment_file(
    _target: usize,
    _path: *const u8,
    _buffer: *mut u8,
) -> u32 {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "mov eax, [ebp + 12]",
        "push dword ptr [ebp + 16]",
        "call dword ptr [ebp + 8]",
        "add esp, 4",
        "pop ebp",
        "ret",
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn load_original(
    _target: usize,
    _index: u32,
    _source: *mut super::Source,
    _root: *const u8,
) -> u32 {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "mov eax, [ebp + 12]",
        "push dword ptr [ebp + 20]",
        "push dword ptr [ebp + 16]",
        "call dword ptr [ebp + 8]",
        "add esp, 8",
        "pop ebp",
        "ret",
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn build_original(
    _target: usize,
    _source: *const super::Source,
    _flags: u32,
    _materials: *const u32,
) -> i32 {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "mov ecx, [ebp + 12]",
        "push dword ptr [ebp + 20]",
        "push dword ptr [ebp + 16]",
        "call dword ptr [ebp + 8]",
        "add esp, 8",
        "pop ebp",
        "ret",
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn schedule(
    _target: usize,
    _callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> i32,
    _argument: *mut std::ffi::c_void,
) -> i32 {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push esi",
        "push edi",
        "mov edi, [ebp + 12]",
        "mov esi, [ebp + 16]",
        "call dword ptr [ebp + 8]",
        "pop edi",
        "pop esi",
        "pop ebp",
        "ret",
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn convert_vertices(
    _target: usize,
    _destination: *mut u8,
    _source: *const u8,
    _fvf: u32,
    _count: u32,
    _format: u32,
) {
    core::arch::naked_asm!(
        "push ebp",
        "mov ebp, esp",
        "push edi",
        "mov eax, [ebp + 12]",
        "mov ecx, [ebp + 16]",
        "mov edi, [ebp + 20]",
        "push dword ptr [ebp + 28]",
        "push dword ptr [ebp + 24]",
        "call dword ptr [ebp + 8]",
        "add esp, 8",
        "pop edi",
        "pop ebp",
        "ret",
    );
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn source_query(_target: usize, _source: *const super::Source) -> u32 {
    core::arch::naked_asm!("mov eax, [esp + 8]", "jmp dword ptr [esp + 4]");
}
