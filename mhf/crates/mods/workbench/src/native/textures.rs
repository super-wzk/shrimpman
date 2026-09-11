//! Asset-owned native texture handles. The 128 dynamic TXB bank entries are
//! lookup slots, not the texture pool: `100113F0` allocates handles 1..4096.
//! `108F88E0` resolves the bank into 140-byte materials only during construction.
//! Borrow one empty bank entry for that constructor, then restore the original
//! material image references to our handles and return the bank entry at once.

use super::Client;
use mhf_resource::{
    dds::{self, Dds},
    fmod::{Fmod, MaterialEntry, Section, TextureEntry},
    png::{self, Png},
    txb::{Image, Txb},
};
use std::{mem::transmute, ptr};
use windows::Win32::System::Threading::{
    CRITICAL_SECTION, EnterCriticalSection, LeaveCriticalSection,
};

const TEXTURE_BANK: usize = 0x1ecd_a320;
const DYNAMIC_BANK: std::ops::Range<usize> = 3900..4028;
const TEXTURE_POOL: usize = 0x11aa_7d80;
const TEXTURE_STRIDE: usize = 216;
const TEXTURE_LOCK: usize = 0x1e73_ac68;
const TEXTURE_COUNT: usize = 0x11b8_c65c;
pub(super) const TEXTURE_CAPACITY: usize = 4095;
const MATERIAL_SIZE: usize = 140;
const IMAGE_FIELDS: [usize; 4] = [68, 76, 80, 84];

/// Typed borrowed images from either a TXB directory or one standalone image.
/// Image headers and trailing bytes stay in the original resource allocation.
pub(super) fn images<'a>(sources: &[&'a [u8]]) -> Result<Vec<Image<'a>>, String> {
    let mut images = Vec::new();
    for &source in sources {
        if source.starts_with(&png::MAGIC) {
            images.push(Image::Png(
                Png::parse(source).map_err(|error| error.to_string())?,
            ));
        } else if source.starts_with(&dds::MAGIC) {
            images.push(Image::Dds(
                Dds::parse(source).map_err(|error| error.to_string())?,
            ));
        } else {
            let bank = Txb::parse(source, source.len()).map_err(|error| error.to_string())?;
            images.extend(bank.textures.into_iter().map(|texture| texture.image));
        }
    }
    Ok(images)
}

pub(super) unsafe fn validate_interfaces(client: Client) -> Result<(), String> {
    // Prefixes stop before relocated absolute operands.
    for (address, prefix) in [
        (0x100113fc, &[0xbe, 1, 0, 0, 0, 0xb8][..]),
        (
            0x10011406,
            &[0x83, 0x38, 0, 0x74, 0x1c, 0x05, 0xd8, 0, 0, 0, 0x46, 0x3d][..],
        ),
        (
            0x10011af0,
            &[
                0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x81, 0xec, 0x8c, 0, 0, 0,
            ][..],
        ),
        (
            0x100119b0,
            &[0x56, 0x8b, 0xf0, 0x81, 0xfe, 0, 0x10, 0, 0, 0x72, 4][..],
        ),
    ] {
        if unsafe { std::slice::from_raw_parts(client.address(address) as *const u8, prefix.len()) }
            != prefix
        {
            return Err(format!("不支持此客户端的贴图句柄接口：{address:#x}"));
        }
    }
    // Verify the actual allocator layout as well as its instruction prefixes.
    for (operand, target) in [
        (0x100113f2, TEXTURE_LOCK),
        (0x10011402, TEXTURE_POOL + TEXTURE_STRIDE),
        (
            0x10011412,
            TEXTURE_POOL + (TEXTURE_CAPACITY + 1) * TEXTURE_STRIDE,
        ),
        (0x10011429, TEXTURE_COUNT),
        (0x10011b29, TEXTURE_POOL),
    ] {
        if unsafe { ptr::read_unaligned(client.address(operand) as *const usize) }
            != client.address(target)
        {
            return Err(format!("不支持此客户端的贴图池布局：{operand:#x}"));
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct TextureHandle {
    index: u32,
    /// Scene/device teardown may clear or reuse this native slot before us.
    texture: usize,
}

impl TextureHandle {
    unsafe fn still_owned(self, client: Client) -> bool {
        (unsafe { client.read::<usize>(TEXTURE_POOL + self.index as usize * TEXTURE_STRIDE) })
            == self.texture
    }
}

#[derive(Default)]
pub(super) struct NativeTextures {
    /// `Resource.model_source` points here until native resource destruction.
    model: Box<[u8]>,
    material_images: Vec<[Option<usize>; 4]>,
    handles: Vec<TextureHandle>,
}

impl NativeTextures {
    pub(super) fn prepare(model: &[u8], texture_count: usize) -> Result<Self, String> {
        let parsed = Fmod::parse(model).map_err(|error| error.to_string())?;
        let mut preview = model.to_vec().into_boxed_slice();
        let mut image_ids = Vec::new();
        for section in &parsed.sections {
            if let Section::Textures(table) = section {
                for entry in &table.records {
                    let TextureEntry::Texture(texture) = entry else {
                        return Err("未知原生贴图引用记录".into());
                    };
                    if texture.image_id as usize >= texture_count {
                        return Err(format!(
                            "贴图 image ID {} 尚未绑定，当前提供 {texture_count} 张图片",
                            texture.image_id
                        ));
                    }
                    image_ids.push(texture.image_id as usize);
                    let offset = texture.block.offset() + 12;
                    preview[offset..offset + 4].copy_from_slice(&0_u32.to_le_bytes());
                }
            }
        }
        let mut material_images = Vec::new();
        for section in &parsed.sections {
            if let Section::Materials(table) = section {
                for entry in &table.records {
                    let MaterialEntry::Material(material) = entry else {
                        return Err("未知原生材质记录".into());
                    };
                    // 100027B0 leaves all four fields zero for an untextured
                    // material; 108F88E0 resolves those to the first bound image.
                    let mut images = if material.texture_indices.is_empty() {
                        [Some(0); 4]
                    } else {
                        [None; 4]
                    };
                    // Native 100027B0 reads four references at most; later
                    // channels stay in the source but do not enter materials.
                    for (channel, &reference) in material.texture_indices.iter().take(4).enumerate()
                    {
                        images[channel] = Some(
                            *image_ids
                                .get(reference as usize)
                                .ok_or("材质贴图引用超出贴图表范围")?,
                        );
                    }
                    material_images.push(images);
                }
            }
        }
        Ok(Self {
            model: preview,
            material_images,
            handles: Vec::new(),
        })
    }

    pub(super) fn model_source(&self) -> usize {
        self.model.as_ptr() as usize
    }

    /// Task thread only, with no overlapping asset load, draw, or teardown.
    pub(super) unsafe fn upload(
        &mut self,
        client: Client,
        sources: &[&[u8]],
    ) -> Result<(), String> {
        let textures = images(sources)?;
        let allocate: unsafe extern "C" fn() -> u32 =
            unsafe { transmute(client.address(0x100113f0)) };
        let upload: unsafe extern "C" fn(usize, u32, u32) -> u32 =
            unsafe { transmute(client.address(0x10011af0)) };
        let result = (|| {
            for (image, texture) in textures.iter().enumerate() {
                let bytes = texture.as_bytes();
                let length = u32::try_from(bytes.len()).map_err(|_| "原生贴图长度超出 DWORD")?;
                let handle = unsafe { allocate() };
                // The native allocator returns 4096 on exhaustion, without
                // reserving it. Never pass that out-of-bounds value to upload.
                if !(1..=TEXTURE_CAPACITY as u32).contains(&handle) {
                    return Err(format!("原生贴图句柄池已满，贴图 {image} 无法分配"));
                }
                let loaded = unsafe { upload(bytes.as_ptr() as usize, length, handle) };
                let pointer = unsafe {
                    client.read::<usize>(TEXTURE_POOL + handle as usize * TEXTURE_STRIDE)
                };
                if loaded != handle || pointer == 0 || pointer == usize::MAX {
                    // Allocation increments the global count before decoding.
                    // Failed decoding leaves no COM object to pass to 100119B0.
                    if pointer == 0 || pointer == usize::MAX {
                        unsafe { cancel_reservation(client, handle) };
                    } else {
                        self.handles.push(TextureHandle {
                            index: handle,
                            texture: pointer,
                        });
                    }
                    return Err(format!("原生贴图 {image} 载入失败"));
                }
                self.handles.push(TextureHandle {
                    index: handle,
                    texture: pointer,
                });
            }
            Ok(())
        })();
        if result.is_err() {
            unsafe { self.release(client) }?;
        }
        result
    }

    /// The constructor receives a count-zero TXB: it creates no texture and
    /// leaves Resource.texture_count zero. A nonzero borrowed bank entry keeps
    /// other native dynamic-bank scans from choosing it during construction.
    pub(super) unsafe fn with_constructor_slot(
        &self,
        client: Client,
        construct: impl FnOnce(i32, usize),
    ) -> Result<(), String> {
        let handle = self
            .handles
            .first()
            .ok_or("原生材质需要默认贴图槽 0，请追加贴图")?
            .index;
        let slot = DYNAMIC_BANK
            .clone()
            .find(|&slot| unsafe { client.read::<u32>(TEXTURE_BANK + 4 * slot) } == 0)
            .ok_or("原生动态贴图区没有用于构建材质的空闲槽位")?;
        let binding = BankBinding {
            address: client.address(TEXTURE_BANK + 4 * slot) as *mut u32,
            handle,
        };
        unsafe { ptr::write(binding.address, handle) };
        let empty_txb = 0_u32;
        construct(slot as i32, &empty_txb as *const u32 as usize);
        drop(binding);
        Ok(())
    }

    pub(super) unsafe fn bind_materials(
        &self,
        materials: usize,
        count: usize,
    ) -> Result<(), String> {
        if materials == 0 || count != self.material_images.len() {
            return Err("原生材质数量与贴图绑定不一致".into());
        }
        for (index, images) in self.material_images.iter().enumerate() {
            unsafe { self.bind_material(materials + MATERIAL_SIZE * index, images) }?;
        }
        Ok(())
    }

    /// 10007B60 flag 2 copies local materials into the registered model. Rigid
    /// models use that copy in 10017D10 instead of the per-draw parameter table.
    pub(super) unsafe fn bind_mesh_materials(
        &self,
        client: Client,
        handle: u32,
        indices: &[u32],
    ) -> Result<(), String> {
        if !(1..2880).contains(&handle) {
            return Err("原生网格句柄无效，无法绑定贴图".into());
        }
        let model = unsafe { client.read::<usize>(0x11aa_3dd8 + 4 * handle as usize) };
        if model == 0 {
            return Err("原生网格已被释放，无法绑定贴图".into());
        }
        let header = unsafe { &*(model as *const [u32; 24]) };
        let Some(offset) = baked_material_offset(header, indices.len())? else {
            return Ok(());
        };
        for (local, &index) in indices.iter().enumerate() {
            let images = self
                .material_images
                .get(index as usize)
                .ok_or("原生网格引用了不存在的材质")?;
            unsafe { self.bind_material(model + offset + MATERIAL_SIZE * local, images) }?;
        }
        Ok(())
    }

    unsafe fn bind_material(
        &self,
        material: usize,
        images: &[Option<usize>; 4],
    ) -> Result<(), String> {
        for (&offset, &image) in IMAGE_FIELDS.iter().zip(images) {
            let handle = image.map_or(Ok(u32::MAX), |image| {
                self.handles
                    .get(image)
                    .map(|handle| handle.index)
                    .ok_or("原生材质引用尚未上传的贴图")
            })?;
            unsafe { ptr::write((material + offset) as *mut u32, handle) };
        }
        Ok(())
    }

    pub(super) unsafe fn validate(&self, client: Client, count: usize) -> Result<(), String> {
        if self.handles.len() != count {
            return Err("原生贴图上传数量不完整".into());
        }
        for (image, &handle) in self.handles.iter().enumerate() {
            if !unsafe { handle.still_owned(client) } {
                return Err(format!("原生贴图 {image} 已被释放或替换"));
            }
        }
        Ok(())
    }

    /// Task thread only, after native meshes and queued draws are released.
    pub(super) unsafe fn release(&mut self, client: Client) -> Result<(), String> {
        while let Some(&handle) = self.handles.last() {
            if unsafe { handle.still_owned(client) }
                && unsafe { release_handle(client.address(0x100119b0), handle.index) } != 1
            {
                return Err(format!("原生贴图句柄 {} 释放失败", handle.index));
            }
            self.handles.pop();
        }
        Ok(())
    }
}

fn baked_material_offset(header: &[u32; 24], count: usize) -> Result<Option<usize>, String> {
    if header[1] & 2 == 0 {
        return Ok(None);
    }
    let offset = header[20] as usize;
    if offset < 96
        || count
            .checked_mul(MATERIAL_SIZE)
            .and_then(|bytes| offset.checked_add(bytes))
            .is_none_or(|end| end > header[3] as usize)
    {
        return Err("原生网格材质副本超出已分配范围".into());
    }
    Ok(Some(offset))
}

struct BankBinding {
    address: *mut u32,
    handle: u32,
}

impl Drop for BankBinding {
    fn drop(&mut self) {
        // Supported task-thread construction is serialized. Keep a changed
        // slot intact even if an unsupported concurrent producer replaced it.
        unsafe {
            if ptr::read(self.address) == self.handle {
                ptr::write(self.address, 0);
            }
        }
    }
}

unsafe fn cancel_reservation(client: Client, handle: u32) {
    let critical = client.address(TEXTURE_LOCK) as *mut CRITICAL_SECTION;
    unsafe {
        EnterCriticalSection(critical);
        ptr::write(
            client.address(TEXTURE_POOL + handle as usize * TEXTURE_STRIDE) as *mut usize,
            0,
        );
        let count = client.address(TEXTURE_COUNT) as *mut u32;
        ptr::write(count, ptr::read(count) - 1);
        LeaveCriticalSection(critical);
    }
}

/// 100119B0 takes EAX and dispatches COM destruction to the render worker.
#[unsafe(naked)]
unsafe extern "C" fn release_handle(_target: usize, _handle: u32) -> i32 {
    core::arch::naked_asm!("mov eax,[esp+8]", "jmp dword ptr [esp+4]");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_dds_and_txb_keep_the_same_borrowed_image_bytes() {
        let mut dds = vec![0_u8; 132];
        dds[..4].copy_from_slice(&dds::MAGIC);
        for (offset, word) in [
            (4, 124_u32),
            (8, 0x1007),
            (12, 1),
            (16, 1),
            (76, 32),
            (80, 0x40),
            (88, 32),
            (108, 0x1000),
        ] {
            dds[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        let direct = images(&[&dds]).unwrap();
        let [Image::Dds(image)] = &direct[..] else {
            panic!("standalone DDS")
        };
        assert_eq!(image.surfaces(image.pixel_data().len()).unwrap().len(), 1);
        assert_eq!(image.as_bytes().as_ptr(), dds.as_ptr());
        assert_eq!(image.as_bytes(), dds);

        let mut bank = [1_u32, 12, dds.len() as u32]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        bank.extend_from_slice(&dds);
        let bundled = images(&[&bank]).unwrap();
        assert!(matches!(&bundled[..], [Image::Dds(_)]));
        assert_eq!(bundled[0].as_bytes().as_ptr(), bank[12..].as_ptr());
        assert_eq!(bundled[0].as_bytes(), direct[0].as_bytes());
        let empty = 0_u32.to_le_bytes();
        let combined = images(&[&empty, &dds, &bank]).unwrap();
        assert_eq!(combined.len(), 2);
        assert_eq!(combined[0].as_bytes().as_ptr(), dds.as_ptr());
        assert_eq!(combined[1].as_bytes().as_ptr(), bank[12..].as_ptr());
    }

    #[test]
    fn empty_bank_requires_a_real_default_image_before_construction() {
        let mut textures = NativeTextures {
            material_images: vec![[Some(0); 4]],
            ..Default::default()
        };
        // An unmapped client proves the empty path never reads a pool, reserves
        // a directory slot or calls a native upload/release function.
        let client = Client { base: 0 };
        let mut material = [0x1234_5678_u32; MATERIAL_SIZE / 4];
        unsafe {
            textures.upload(client, &[&0_u32.to_le_bytes()]).unwrap();
            assert!(
                textures
                    .with_constructor_slot(client, |_, _| panic!(
                        "missing default image must not construct"
                    ))
                    .is_err()
            );
            assert!(
                textures
                    .bind_materials(material.as_mut_ptr() as usize, 1)
                    .is_err()
            );
            textures.validate(client, 0).unwrap();
            textures.release(client).unwrap();
        }
        assert_eq!(material, [0x1234_5678; MATERIAL_SIZE / 4]);
    }

    #[test]
    fn temporary_bank_binding_does_not_clear_another_owner() {
        let mut bank = 12_u32;
        drop(BankBinding {
            address: &mut bank,
            handle: 12,
        });
        assert_eq!(bank, 0);
        bank = 31;
        drop(BankBinding {
            address: &mut bank,
            handle: 12,
        });
        assert_eq!(bank, 31);
    }

    #[test]
    fn rigid_material_copies_must_fit_the_registered_model_allocation() {
        let mut header = [0; 24];
        header[1] = 4;
        assert_eq!(baked_material_offset(&header, 3).unwrap(), None);
        header[1] = 6;
        header[20] = 400;
        header[3] = 400 + 3 * MATERIAL_SIZE as u32;
        assert_eq!(baked_material_offset(&header, 3).unwrap(), Some(400));
        assert!(baked_material_offset(&header, 4).is_err());
        assert!(baked_material_offset(&header, usize::MAX).is_err());
        header[20] = 80;
        assert!(baked_material_offset(&header, 1).is_err());
    }

    #[test]
    fn material_rebinding_preserves_other_fields_and_handles_more_than_128_images() {
        let textures = NativeTextures {
            material_images: vec![[Some(140), None, Some(128), Some(0)]],
            handles: (0..141)
                .map(|index| TextureHandle {
                    index: index + 100,
                    texture: index as usize + 2000,
                })
                .collect(),
            ..Default::default()
        };
        let mut material = [0x1234_5678_u32; MATERIAL_SIZE / 4];
        unsafe {
            textures
                .bind_materials(material.as_mut_ptr() as usize, 1)
                .unwrap();
        }
        for (index, &value) in material.iter().enumerate() {
            assert_eq!(
                value,
                match 4 * index {
                    68 => 240,
                    76 => u32::MAX,
                    80 => 228,
                    84 => 100,
                    _ => 0x1234_5678,
                }
            );
        }
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn actual_character_editor_banks_keep_all_141_images_and_source_bytes() {
        use crate::{inspect, preview::AssetBundle};
        use std::{collections::BTreeSet, sync::Arc};
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        for name in ["dat/parts/f00/f_editpl.bin", "dat/parts/m00/m_editpl.bin"] {
            let document = Arc::new(inspect::inspect(
                name,
                std::fs::read(root.join(name)).unwrap().into(),
            ));
            let bundles = AssetBundle::find_with_nodes(document).0;
            assert_eq!(bundles.len(), 1, "{name}");
            let bundle = &bundles[0];
            super::super::asset::preflight(bundle).unwrap();
            let model = bundle.model.bytes().unwrap();
            let original = model.to_vec();
            let bank = Txb::parse(bundle.textures[0].bytes().unwrap(), TEXTURE_CAPACITY).unwrap();
            let textures = NativeTextures::prepare(model, bank.textures.len()).unwrap();
            let used: BTreeSet<_> = textures
                .material_images
                .iter()
                .flatten()
                .flatten()
                .collect();
            eprintln!(
                "{name}: bank={} material_channels={} unique_images={} IDs={used:?}",
                bank.textures.len(),
                textures.material_images.iter().flatten().flatten().count(),
                used.len(),
            );
            assert_eq!(bank.textures.len(), 141);
            assert_eq!(model, original);
            assert_ne!(textures.model_source(), model.as_ptr() as usize);
            let prepared = Fmod::parse(&textures.model).unwrap();
            for section in prepared.sections {
                if let Section::Textures(table) = section {
                    for entry in table.records {
                        let TextureEntry::Texture(texture) = entry else {
                            panic!()
                        };
                        assert_eq!(texture.image_id, 0);
                    }
                }
            }
        }
    }
}
