use super::{Client, validate};
use mhf_hooks::{HookSlot, ModuleReference};
use std::{ffi::c_void, ptr};
use windows::{
    Win32::System::{
        Diagnostics::Debug::FlushInstructionCache,
        LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW},
        Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect},
        Threading::GetCurrentProcess,
    },
    core::PCWSTR,
};

static HOOKS: HookSlot<()> = HookSlot::new();

// The test never starts the game or calls these entrypoints. Trap if that
// invariant changes instead of returning through an unrelated native ABI.
#[unsafe(naked)]
unsafe extern "C" fn unexpected_call() {
    core::arch::naked_asm!("ud2");
}

struct ChangedByte {
    address: *mut u8,
    original: u8,
    protection: PAGE_PROTECTION_FLAGS,
}

impl ChangedByte {
    unsafe fn new(address: usize) -> Self {
        let address = address as *mut u8;
        let original = unsafe { ptr::read(address) };
        let mut protection = PAGE_PROTECTION_FLAGS::default();
        unsafe { VirtualProtect(address.cast(), 1, PAGE_EXECUTE_READWRITE, &mut protection) }
            .expect("make the test byte writable");
        let change = Self {
            address,
            original,
            protection,
        };
        unsafe {
            ptr::write(address, original ^ 1);
            FlushInstructionCache(GetCurrentProcess(), Some(address.cast()), 1)
        }
        .expect("flush the changed test byte");
        change
    }
}

impl Drop for ChangedByte {
    fn drop(&mut self) {
        let mut ignored = PAGE_PROTECTION_FLAGS::default();
        unsafe {
            ptr::write(self.address, self.original);
            FlushInstructionCache(GetCurrentProcess(), Some(self.address.cast()), 1)
                .expect("flush the restored test byte");
            VirtualProtect(self.address.cast(), 1, self.protection, &mut ignored)
                .expect("restore the test byte's protection");
        }
    }
}

#[test]
#[ignore = "set MHF_TEST_CLIENT; loads the real game DLL and its DllMain"]
fn supported_client_accepts_geometry_hooks_and_rejects_changed_interfaces() {
    let path = std::env::var("MHF_TEST_CLIENT").expect("MHF_TEST_CLIENT");
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    let module =
        unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
            .expect("load supported game DLL and adjacent dependencies");
    let _loaded = unsafe { ModuleReference::from_owned(module) };
    let client = Client {
        base: module.0 as usize,
    };
    let entries = [0x108f88e0, 0x108f8eb0, 0x10011af0, 0x100113f0];
    let read_entry =
        |address| unsafe { ptr::read_unaligned(client.address(address) as *const [u8; 12]) };
    let originals = entries.map(read_entry);
    unsafe { validate(client) }.expect("validate the unmodified client");

    let mut pending = HOOKS.prepare().expect("prepare test hooks");
    for address in entries {
        unsafe {
            pending.create(
                "workbench geometry interface regression",
                client.address(address) as *mut c_void,
                unexpected_call as *mut c_void,
            )
        }
        .expect("create the same entry hook as geometry");
    }
    let mut hooks = unsafe { pending.install(()) }.expect("enable the geometry entry hooks");
    for (address, original) in entries.into_iter().zip(&originals) {
        assert_ne!(
            read_entry(address),
            *original,
            "entry {address:#x} was not hooked"
        );
    }
    unsafe { validate(client) }.expect("accept supported client with geometry hooks active");

    for (address, expected_error) in [
        (0x108f88e6, "不支持此客户端的资源预览接口"),
        (0x10011402, "不支持此客户端的贴图池布局"),
    ] {
        {
            let _changed = unsafe { ChangedByte::new(client.address(address)) };
            let error = unsafe { validate(client) }.expect_err("reject changed native interface");
            assert!(
                error.contains(expected_error),
                "unexpected rejection: {error}"
            );
        }
        unsafe { validate(client) }.expect("accept restored native interface while hooked");
    }

    hooks.uninstall().expect("remove test hooks");
    for (address, original) in entries.into_iter().zip(&originals) {
        assert_eq!(
            read_entry(address),
            *original,
            "entry {address:#x} was not restored"
        );
    }
    unsafe { validate(client) }.expect("accept the client after all hooks are removed");
}
