//! Runs the original file/decryption/relocation path in an isolated test process.

use super::{
    MAIN_RESOURCE_BINDINGS, MemoryRange, RESOURCE_LAYOUTS, RecordCount, ResourceBodyLayout,
};
use std::{ffi::CStr, path::PathBuf, ptr};
use windows::{
    Win32::System::LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW},
    core::PCWSTR,
};

type Loader = unsafe extern "C" fn(*mut u32) -> usize;
type Free = unsafe extern "C" fn(*mut std::ffi::c_void);

struct ClientResource {
    id: &'static str,
    loader_rva: usize,
    bytes: usize,
    cells: usize,
    nonnull: usize,
    sample: &'static str,
}

const RESOURCES: [ClientResource; 5] = [
    ClientResource {
        id: "mhfjmp",
        loader_rva: 0x001C_C740,
        bytes: 3296,
        cells: 53,
        nonnull: 53,
        sample: "移動メニュー",
    },
    ClientResource {
        id: "mhfrcc",
        loader_rva: 0x00AF_C870,
        bytes: 1728,
        cells: 36,
        nonnull: 36,
        sample: "現在、極限征伐戦が開催中です！",
    },
    ClientResource {
        id: "mhfgao",
        loader_rva: 0x00AF_A670,
        bytes: 210176,
        cells: 4500,
        nonnull: 2881,
        sample: "どんぐりネコヘルム",
    },
    ClientResource {
        id: "mhfmsx",
        loader_rva: 0x00AF_B570,
        bytes: 15072,
        cells: 40,
        nonnull: 40,
        sample: "輝く杯",
    },
    ClientResource {
        id: "mhfsqd",
        loader_rva: 0x001C_CF70,
        bytes: 10168,
        cells: 255,
        nonnull: 253,
        sample: "高速剥ぎ取り＆採取",
    },
];

struct WorkingDirectory(PathBuf);

impl Drop for WorkingDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore test working directory");
    }
}

struct LoadedResources {
    base: usize,
    // Resource globals may already contain allocations owned by another test.
    saved: Vec<(usize, u32, u32)>,
    weapon_types: Vec<u8>,
}

impl LoadedResources {
    unsafe fn new(base: usize) -> Self {
        let saved = RESOURCES
            .iter()
            .map(|resource| {
                let index = MAIN_RESOURCE_BINDINGS
                    .iter()
                    .position(|binding| binding.id == resource.id)
                    .expect("all five resources have loader bindings");
                let binding = &MAIN_RESOURCE_BINDINGS[index];
                unsafe {
                    (
                        index,
                        ptr::read((base + binding.buffer_rva) as *const u32),
                        ptr::read((base + binding.size_rva) as *const u32),
                    )
                }
            })
            .collect();
        // GAO's final 10913E00 helper only builds this numeric weapon-type map.
        let weapon_types =
            unsafe { std::slice::from_raw_parts((base + 0x0DC0_07C8) as *const u8, 1024) }.to_vec();
        Self {
            base,
            saved,
            weapon_types,
        }
    }
}

impl Drop for LoadedResources {
    fn drop(&mut self) {
        let free: Free = unsafe { std::mem::transmute(self.base + 0x015A_B644) };
        for &(index, original, size) in &self.saved {
            let binding = &MAIN_RESOURCE_BINDINGS[index];
            let buffer = (self.base + binding.buffer_rva) as *mut u32;
            unsafe {
                let loaded = ptr::read(buffer);
                ptr::write(buffer, original);
                ptr::write((self.base + binding.size_rva) as *mut u32, size);
                if loaded != 0 && loaded != original {
                    free(loaded as *mut _);
                }
            }
        }
        unsafe {
            ptr::copy_nonoverlapping(
                self.weapon_types.as_ptr(),
                (self.base + 0x0DC0_07C8) as *mut u8,
                self.weapon_types.len(),
            );
        }
    }
}

unsafe fn word(image: MemoryRange, address: usize, width: usize) -> u32 {
    assert!(address >= image.start && address + width <= image.end);
    unsafe {
        match width {
            1 => u32::from(ptr::read(address as *const u8)),
            2 => u32::from(ptr::read_unaligned(address as *const u16)),
            4 => ptr::read_unaligned(address as *const u32),
            _ => panic!("unsupported count width {width}"),
        }
    }
}

unsafe fn field(image: MemoryRange, path: &[u32]) -> usize {
    let (&last, parents) = path.split_last().expect("nonempty structural path");
    let mut address = image.start;
    for &offset in parents {
        address = unsafe { word(image, address + offset as usize, 4) } as usize;
        assert!((image.start..image.end).contains(&address));
    }
    address + last as usize
}

unsafe fn table_pointer(image: MemoryRange, path: &[u32]) -> usize {
    let address = unsafe { field(image, path) };
    let target = unsafe { word(image, address, 4) } as usize;
    assert!((image.start..image.end).contains(&target));
    target
}

unsafe fn records(image: MemoryRange, count: RecordCount) -> usize {
    unsafe {
        match count {
            RecordCount::Fixed(count) => count as usize,
            RecordCount::U16(path) => word(image, field(image, path), 2) as usize,
            RecordCount::U32(path) => word(image, field(image, path), 4) as usize,
            RecordCount::Sentinel {
                root,
                stride,
                offset,
                width,
                value,
            } => {
                let start = table_pointer(image, root);
                assert_ne!(stride, 0);
                for count in 0..(image.end - start) / usize::from(stride) {
                    let address = start + count * usize::from(stride) + usize::from(offset);
                    if word(image, address, usize::from(width)) == value {
                        return count;
                    }
                }
                panic!("unterminated count table");
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TextCell {
    address: usize,
    pointer: usize,
    text: Option<String>,
}

unsafe fn snapshot(resource: &ClientResource, image: MemoryRange) -> Vec<TextCell> {
    let layout = RESOURCE_LAYOUTS
        .iter()
        .find(|layout| layout.id == resource.id)
        .expect("resource layout");
    let ResourceBodyLayout::Records(tables) = layout.body else {
        panic!("extra resource must use record tables");
    };
    let mut result = Vec::new();
    for table in tables {
        if let Some((index, count)) = table.directory
            && index as usize >= unsafe { records(image, count) }
        {
            continue;
        }
        let count = unsafe { records(image, table.records) };
        if count == 0 {
            continue;
        }
        let start = unsafe { table_pointer(image, table.root) };
        for record in 0..count {
            for part in 0..usize::from(table.parts) {
                let address = start
                    + (table.first_record as usize + record) * usize::from(table.stride)
                    + usize::from(table.text_offset)
                    + part * 4;
                let pointer = unsafe { word(image, address, 4) } as usize;
                let text = if pointer == 0 || pointer == image.start {
                    None
                } else {
                    Some(
                        unsafe { CStr::from_ptr(pointer as *const i8) }
                            .to_str()
                            .unwrap_or_else(|error| {
                                panic!(
                                    "{}:{}:{record}:{part} is not UTF-8: {error}",
                                    resource.id, table.id
                                )
                            })
                            .to_owned(),
                    )
                };
                result.push(TextCell {
                    address,
                    pointer,
                    text,
                });
            }
        }
    }
    result
}

#[test]
#[ignore = "requires MHF_UTF8_TEST_CLIENT, the original dat files, and --test-threads=1"]
fn supported_client_loads_all_five_extra_resources_as_utf8() {
    let path =
        PathBuf::from(std::env::var_os("MHF_UTF8_TEST_CLIENT").expect("MHF_UTF8_TEST_CLIENT"));
    assert!(path.is_absolute(), "use an absolute DLL path");
    let directory = path.parent().expect("DLL parent directory");
    for resource in &RESOURCES {
        let bytes = std::fs::read(directory.join("dat").join(format!("{}.bin", resource.id)))
            .expect("original game resource is readable");
        assert!(
            bytes.len() >= 32 && &bytes[..4] == b"ecd\x1a",
            "{} ECD header",
            resource.id
        );
    }
    let _directory = WorkingDirectory(std::env::current_dir().expect("current directory"));
    std::env::set_current_dir(directory).expect("use the game's read-only dat directory");
    let wide = path
        .to_str()
        .expect("Unicode DLL path")
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let module =
        unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
            .expect("load game DLL and initialize its native CRT");
    let _module = unsafe { super::ModuleReference::from_owned(module) };
    let base = module.0 as usize;
    // An empty registry selects direct files. Do not rewrite live package state.
    assert!(
        unsafe { std::slice::from_raw_parts((base + 0x0E83_1E00) as *const u32, 77) }
            .iter()
            .all(|entry| *entry == 0),
        "run in a fresh process before the game resource registry initializes"
    );

    for round in 0..2 {
        // Declare allocations first: hooks must be removed before native buffers
        // and their globals are released, including when an assertion unwinds.
        let loaded = unsafe { LoadedResources::new(base) };
        let mut resources =
            unsafe { super::install(module, None) }.expect("install resource hooks");
        for resource in &RESOURCES {
            let index = MAIN_RESOURCE_BINDINGS
                .iter()
                .position(|binding| binding.id == resource.id)
                .unwrap();
            let binding = &MAIN_RESOURCE_BINDINGS[index];
            let loader: Loader = unsafe { std::mem::transmute(base + resource.loader_rva) };
            let mut status = u32::MAX;
            eprintln!("native resource round {round}: {}", resource.id);
            unsafe { loader(&raw mut status) };
            assert_eq!(status, 0, "{} native load status", resource.id);
            let start = unsafe { ptr::read((base + binding.buffer_rva) as *const u32) } as usize;
            let size = unsafe { ptr::read((base + binding.size_rva) as *const u32) } as usize;
            assert_ne!(start, 0, "{} native buffer", resource.id);
            assert_eq!(size, resource.bytes, "{} decoded bytes", resource.id);
            let image = MemoryRange {
                start,
                end: start + size,
            };
            let first = unsafe { snapshot(resource, image) };
            assert_eq!(
                first.len(),
                resource.cells,
                "{} declared text cells",
                resource.id
            );
            assert_eq!(
                first.iter().filter(|cell| cell.text.is_some()).count(),
                resource.nonnull,
                "{} nonnull text cells",
                resource.id
            );
            assert!(
                first
                    .iter()
                    .any(|cell| cell.text.as_deref() == Some(resource.sample)),
                "{} representative original text",
                resource.id
            );
            if resource.id == "mhfsqd" {
                let nulls = first
                    .iter()
                    .filter(|cell| cell.text.is_none())
                    .collect::<Vec<_>>();
                assert_eq!(nulls.len(), 2);
                assert!(
                    nulls.iter().all(|cell| cell.pointer == image.start),
                    "SQD's relocated zero offsets must stay equal to its image base"
                );
            }
            unsafe { super::patch_resource_dispatch(index as u32) };
            assert_eq!(
                unsafe { snapshot(resource, image) },
                first,
                "{} repeated patch must preserve UTF-8 and pointer identity",
                resource.id
            );
        }
        resources.uninstall().expect("remove resource hooks");
        drop(resources);
        drop(loaded);
    }
}
