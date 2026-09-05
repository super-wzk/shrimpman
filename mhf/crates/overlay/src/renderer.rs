use std::collections::HashMap;
use std::ffi::c_void;
use std::mem;
use std::ptr;

use egui::epaint::{ClippedPrimitive, ImageDelta, Primitive};
use egui::{Color32, TextureFilter, TextureId, TextureWrapMode, TexturesDelta};
use windows::Win32::Foundation::{HANDLE, RECT};
use windows::Win32::Graphics::Direct3D9::{
    D3DBACKBUFFER_TYPE_MONO, D3DBLEND_INVSRCALPHA, D3DBLEND_ONE, D3DBLENDOP_ADD, D3DCULL_NONE,
    D3DFILL_SOLID, D3DFMT_A8R8G8B8, D3DFMT_INDEX32, D3DFVF_DIFFUSE, D3DFVF_TEX1, D3DFVF_XYZ,
    D3DLOCK_DISCARD, D3DLOCKED_RECT, D3DPOOL_DEFAULT, D3DPT_TRIANGLELIST, D3DRS_ALPHABLENDENABLE,
    D3DRS_ALPHATESTENABLE, D3DRS_BLENDOP, D3DRS_BLENDOPALPHA, D3DRS_CLIPPING,
    D3DRS_COLORWRITEENABLE, D3DRS_CULLMODE, D3DRS_DESTBLEND, D3DRS_DESTBLENDALPHA, D3DRS_FILLMODE,
    D3DRS_FOGENABLE, D3DRS_LIGHTING, D3DRS_SCISSORTESTENABLE, D3DRS_SEPARATEALPHABLENDENABLE,
    D3DRS_SHADEMODE, D3DRS_SRCBLEND, D3DRS_SRCBLENDALPHA, D3DRS_SRGBWRITEENABLE,
    D3DRS_STENCILENABLE, D3DRS_ZENABLE, D3DRS_ZWRITEENABLE, D3DSAMP_ADDRESSU, D3DSAMP_ADDRESSV,
    D3DSAMP_ADDRESSW, D3DSAMP_MAGFILTER, D3DSAMP_MINFILTER, D3DSAMP_MIPFILTER, D3DSBT_ALL,
    D3DSHADE_GOURAUD, D3DSURFACE_DESC, D3DTA_DIFFUSE, D3DTA_TEXTURE, D3DTADDRESS_CLAMP,
    D3DTADDRESS_MIRROR, D3DTADDRESS_WRAP, D3DTEXF_LINEAR, D3DTEXF_NONE, D3DTEXF_POINT,
    D3DTOP_DISABLE, D3DTOP_MODULATE, D3DTRANSFORMSTATETYPE, D3DTS_PROJECTION, D3DTS_VIEW,
    D3DTSS_ALPHAARG1, D3DTSS_ALPHAARG2, D3DTSS_ALPHAOP, D3DTSS_COLORARG1, D3DTSS_COLORARG2,
    D3DTSS_COLOROP, D3DUSAGE_DYNAMIC, D3DUSAGE_WRITEONLY, D3DVIEWPORT9, IDirect3DDevice9,
    IDirect3DIndexBuffer9, IDirect3DStateBlock9, IDirect3DSurface9, IDirect3DTexture9,
    IDirect3DVertexBuffer9,
};
use windows_numerics::Matrix4x4;

use crate::{Error, Result};

const INITIAL_VERTEX_CAPACITY: usize = 5_000;
const INITIAL_INDEX_CAPACITY: usize = 10_000;
const D3DFVF_EGUI_VERTEX: u32 = D3DFVF_XYZ | D3DFVF_DIFFUSE | D3DFVF_TEX1;
const D3DTS_WORLD_MATRIX: D3DTRANSFORMSTATETYPE = D3DTRANSFORMSTATETYPE(256);

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    position: [f32; 3],
    color: [u8; 4],
    uv: [f32; 2],
}

impl From<&egui::epaint::Vertex> for Vertex {
    fn from(vertex: &egui::epaint::Vertex) -> Self {
        let [red, green, blue, alpha] = vertex.color.to_array();
        Self {
            position: [vertex.pos.x, vertex.pos.y, 0.0],
            color: [blue, green, red, alpha],
            uv: [vertex.uv.x, vertex.uv.y],
        }
    }
}

struct DrawCall {
    texture_id: TextureId,
    clip_rect: RECT,
    base_vertex: i32,
    vertex_count: u32,
    index_offset: u32,
    index_count: u32,
}

#[derive(Default)]
pub(super) struct Renderer {
    vertex_buffer: Option<IDirect3DVertexBuffer9>,
    vertex_capacity: usize,
    index_buffer: Option<IDirect3DIndexBuffer9>,
    index_capacity: usize,
    textures: HashMap<TextureId, Texture>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    draw_calls: Vec<DrawCall>,
}

impl Renderer {
    pub(super) fn prepare_for_reset(&mut self) {
        self.vertex_buffer = None;
        self.index_buffer = None;
        for texture in self.textures.values_mut() {
            texture.handle = None;
        }
    }

    pub(super) fn paint(
        &mut self,
        device: &IDirect3DDevice9,
        primitives: Vec<ClippedPrimitive>,
        textures_delta: &mut TexturesDelta,
        pixels_per_point: f32,
    ) -> Result<()> {
        let set = mem::take(&mut textures_delta.set);
        for (texture_id, deltas) in set {
            for delta in deltas {
                self.apply_texture_delta(texture_id, &delta)?;
            }
        }

        let paint_result = self.paint_primitives(device, primitives, pixels_per_point);

        for texture_id in mem::take(&mut textures_delta.free) {
            self.textures.remove(&texture_id);
        }

        paint_result
    }

    fn paint_primitives(
        &mut self,
        device: &IDirect3DDevice9,
        primitives: Vec<ClippedPrimitive>,
        pixels_per_point: f32,
    ) -> Result<()> {
        let backbuffer = unsafe { device.GetBackBuffer(0, 0, D3DBACKBUFFER_TYPE_MONO) }
            .map_err(|error| d3d_error("IDirect3DDevice9::GetBackBuffer", error))?;
        let mut description = D3DSURFACE_DESC::default();
        unsafe { backbuffer.GetDesc(&mut description) }
            .map_err(|error| d3d_error("IDirect3DSurface9::GetDesc", error))?;

        let pixels_per_point = pixels_per_point.max(f32::EPSILON);
        let screen_points = [
            description.Width as f32 / pixels_per_point,
            description.Height as f32 / pixels_per_point,
        ];
        collect_meshes(
            primitives,
            pixels_per_point,
            [description.Width, description.Height],
            &mut self.vertices,
            &mut self.indices,
            &mut self.draw_calls,
        )?;
        if self.draw_calls.is_empty() {
            return Ok(());
        }

        self.ensure_device_objects(device, self.vertices.len(), self.indices.len())?;
        upload_vertex_buffer(
            self.vertex_buffer
                .as_ref()
                .expect("vertex buffer must exist after allocation"),
            &self.vertices,
        )?;
        upload_index_buffer(
            self.index_buffer
                .as_ref()
                .expect("index buffer must exist after allocation"),
            &self.indices,
        )?;

        let mut backup = StateBackup::capture(device)?;
        let render_result = (|| {
            unsafe {
                device
                    .SetRenderTarget(0, &backbuffer)
                    .map_err(|error| d3d_error("IDirect3DDevice9::SetRenderTarget", error))?;
                device
                    .BeginScene()
                    .map_err(|error| d3d_error("IDirect3DDevice9::BeginScene", error))?;
            }

            let draw_result = self.draw(
                device,
                &self.draw_calls,
                screen_points,
                [description.Width, description.Height],
                pixels_per_point,
            );
            let end_result = unsafe { device.EndScene() }
                .map_err(|error| d3d_error("IDirect3DDevice9::EndScene", error));
            draw_result.and(end_result)
        })();
        let restore_result = backup.restore();

        render_result.and(restore_result)
    }

    fn draw(
        &self,
        device: &IDirect3DDevice9,
        draw_calls: &[DrawCall],
        screen_points: [f32; 2],
        screen_pixels: [u32; 2],
        pixels_per_point: f32,
    ) -> Result<()> {
        unsafe {
            setup_render_state(device, screen_points, screen_pixels, pixels_per_point)?;
            device
                .SetStreamSource(
                    0,
                    self.vertex_buffer
                        .as_ref()
                        .expect("vertex buffer must exist while drawing"),
                    0,
                    mem::size_of::<Vertex>() as u32,
                )
                .map_err(|error| d3d_error("IDirect3DDevice9::SetStreamSource", error))?;
            device
                .SetIndices(
                    self.index_buffer
                        .as_ref()
                        .expect("index buffer must exist while drawing"),
                )
                .map_err(|error| d3d_error("IDirect3DDevice9::SetIndices", error))?;
        }

        let mut bound_texture = None;
        for call in draw_calls {
            if bound_texture != Some(call.texture_id) {
                let texture = self.textures.get(&call.texture_id).ok_or_else(|| {
                    Error::new(format!(
                        "egui referenced unknown texture {:?}",
                        call.texture_id
                    ))
                })?;
                let handle = texture.handle.as_ref().ok_or_else(|| {
                    Error::new(format!(
                        "egui texture {:?} has no D3D9 resource",
                        call.texture_id
                    ))
                })?;
                unsafe {
                    device
                        .SetTexture(0, handle)
                        .map_err(|error| d3d_error("IDirect3DDevice9::SetTexture", error))?;
                    set_sampler_state(device, texture.options)?;
                }
                bound_texture = Some(call.texture_id);
            }

            unsafe {
                device
                    .SetScissorRect(&call.clip_rect)
                    .map_err(|error| d3d_error("IDirect3DDevice9::SetScissorRect", error))?;
                device
                    .DrawIndexedPrimitive(
                        D3DPT_TRIANGLELIST,
                        call.base_vertex,
                        0,
                        call.vertex_count,
                        call.index_offset,
                        call.index_count / 3,
                    )
                    .map_err(|error| d3d_error("IDirect3DDevice9::DrawIndexedPrimitive", error))?;
            }
        }

        Ok(())
    }

    fn ensure_device_objects(
        &mut self,
        device: &IDirect3DDevice9,
        vertex_count: usize,
        index_count: usize,
    ) -> Result<()> {
        if self.vertex_buffer.is_none() || self.vertex_capacity < vertex_count {
            let capacity = grown_capacity(INITIAL_VERTEX_CAPACITY, vertex_count)?;
            let buffer = create_vertex_buffer(device, capacity)?;
            self.vertex_capacity = capacity;
            self.vertex_buffer = Some(buffer);
        }
        if self.index_buffer.is_none() || self.index_capacity < index_count {
            let capacity = grown_capacity(INITIAL_INDEX_CAPACITY, index_count)?;
            let buffer = create_index_buffer(device, capacity)?;
            self.index_capacity = capacity;
            self.index_buffer = Some(buffer);
        }
        for texture in self.textures.values_mut() {
            texture.ensure_uploaded(device)?;
        }
        Ok(())
    }

    fn apply_texture_delta(&mut self, texture_id: TextureId, delta: &ImageDelta) -> Result<()> {
        let pixels = image_pixels(&delta.image);
        let size = delta.image.size();

        if let Some([x, y]) = delta.pos {
            let texture = self.textures.get_mut(&texture_id).ok_or_else(|| {
                Error::new(format!(
                    "egui sent a partial update for unknown texture {texture_id:?}"
                ))
            })?;
            texture.patch([x, y], size, &pixels)?;
            texture.options = delta.options;
        } else {
            let texture = Texture {
                handle: None,
                size,
                pixels,
                options: delta.options,
            };
            self.textures.insert(texture_id, texture);
        }
        Ok(())
    }
}

fn collect_meshes(
    primitives: Vec<ClippedPrimitive>,
    pixels_per_point: f32,
    screen_pixels: [u32; 2],
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    draw_calls: &mut Vec<DrawCall>,
) -> Result<()> {
    vertices.clear();
    indices.clear();
    draw_calls.clear();

    for primitive in primitives {
        let Primitive::Mesh(mesh) = primitive.primitive else {
            continue;
        };
        if mesh.vertices.is_empty() || mesh.indices.is_empty() {
            continue;
        }
        if mesh.indices.len() % 3 != 0 {
            return Err(Error::new(
                "egui mesh index count is not divisible by three",
            ));
        }

        let base_vertex = i32::try_from(vertices.len())
            .map_err(|_| Error::new("egui base vertex exceeds D3D9 limits"))?;
        let index_offset = u32::try_from(indices.len())
            .map_err(|_| Error::new("egui index offset exceeds D3D9 limits"))?;
        let vertex_count = u32::try_from(mesh.vertices.len())
            .map_err(|_| Error::new("egui vertex count exceeds D3D9 limits"))?;
        let index_count = u32::try_from(mesh.indices.len())
            .map_err(|_| Error::new("egui index count exceeds D3D9 limits"))?;
        let Some(clip_rect) = clip_rect(primitive.clip_rect, pixels_per_point, screen_pixels)
        else {
            continue;
        };

        vertices.extend(mesh.vertices.iter().map(Vertex::from));
        indices.extend_from_slice(&mesh.indices);
        draw_calls.push(DrawCall {
            texture_id: mesh.texture_id,
            clip_rect,
            base_vertex,
            vertex_count,
            index_offset,
            index_count,
        });
    }

    Ok(())
}

fn clip_rect(rect: egui::Rect, pixels_per_point: f32, screen: [u32; 2]) -> Option<RECT> {
    let left = (rect.left() * pixels_per_point)
        .floor()
        .clamp(0.0, screen[0] as f32) as i32;
    let top = (rect.top() * pixels_per_point)
        .floor()
        .clamp(0.0, screen[1] as f32) as i32;
    let right = (rect.right() * pixels_per_point)
        .ceil()
        .clamp(0.0, screen[0] as f32) as i32;
    let bottom = (rect.bottom() * pixels_per_point)
        .ceil()
        .clamp(0.0, screen[1] as f32) as i32;
    (right > left && bottom > top).then_some(RECT {
        left,
        top,
        right,
        bottom,
    })
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Bgra {
    blue: u8,
    green: u8,
    red: u8,
    alpha: u8,
}

impl From<Color32> for Bgra {
    fn from(color: Color32) -> Self {
        let [red, green, blue, alpha] = color.to_array();
        Self {
            blue,
            green,
            red,
            alpha,
        }
    }
}

struct Texture {
    handle: Option<IDirect3DTexture9>,
    size: [usize; 2],
    pixels: Vec<Bgra>,
    options: egui::TextureOptions,
}

impl Texture {
    fn patch(&mut self, position: [usize; 2], size: [usize; 2], pixels: &[Bgra]) -> Result<()> {
        let [x, y] = position;
        let [width, height] = size;
        let right = x
            .checked_add(width)
            .ok_or_else(|| Error::new("egui texture patch overflows horizontally"))?;
        let bottom = y
            .checked_add(height)
            .ok_or_else(|| Error::new("egui texture patch overflows vertically"))?;
        if right > self.size[0] || bottom > self.size[1] {
            return Err(Error::new(format!(
                "egui texture patch [{x}, {y}]..[{right}, {bottom}] exceeds {}x{} texture",
                self.size[0], self.size[1]
            )));
        }
        let expected_pixels = width
            .checked_mul(height)
            .ok_or_else(|| Error::new("egui texture patch dimensions overflow"))?;
        if pixels.len() != expected_pixels {
            return Err(Error::new("egui texture patch has an invalid pixel count"));
        }

        for row in 0..height {
            let source = &pixels[row * width..(row + 1) * width];
            let destination_start = (y + row) * self.size[0] + x;
            self.pixels[destination_start..destination_start + width].copy_from_slice(source);
        }
        self.handle = None;
        Ok(())
    }

    fn ensure_uploaded(&mut self, device: &IDirect3DDevice9) -> Result<()> {
        if self.handle.is_none() {
            let handle = create_texture(device, self.size)?;
            upload_texture(&handle, self.size, &self.pixels)?;
            self.handle = Some(handle);
        }
        Ok(())
    }
}

fn image_pixels(image: &egui::ImageData) -> Vec<Bgra> {
    match image {
        egui::ImageData::Color(image) => image.pixels.iter().copied().map(Bgra::from).collect(),
    }
}

fn create_vertex_buffer(
    device: &IDirect3DDevice9,
    capacity: usize,
) -> Result<IDirect3DVertexBuffer9> {
    let bytes = byte_len::<Vertex>(capacity, "vertex buffer")?;
    let mut buffer = None;
    unsafe {
        device.CreateVertexBuffer(
            bytes,
            (D3DUSAGE_DYNAMIC | D3DUSAGE_WRITEONLY) as u32,
            D3DFVF_EGUI_VERTEX,
            D3DPOOL_DEFAULT,
            &mut buffer,
            ptr::null_mut::<HANDLE>(),
        )
    }
    .map_err(|error| d3d_error("IDirect3DDevice9::CreateVertexBuffer", error))?;
    buffer.ok_or_else(|| Error::new("D3D9 returned a null vertex buffer"))
}

fn create_index_buffer(
    device: &IDirect3DDevice9,
    capacity: usize,
) -> Result<IDirect3DIndexBuffer9> {
    let bytes = byte_len::<u32>(capacity, "index buffer")?;
    let mut buffer = None;
    unsafe {
        device.CreateIndexBuffer(
            bytes,
            (D3DUSAGE_DYNAMIC | D3DUSAGE_WRITEONLY) as u32,
            D3DFMT_INDEX32,
            D3DPOOL_DEFAULT,
            &mut buffer,
            ptr::null_mut::<HANDLE>(),
        )
    }
    .map_err(|error| d3d_error("IDirect3DDevice9::CreateIndexBuffer", error))?;
    buffer.ok_or_else(|| Error::new("D3D9 returned a null index buffer"))
}

fn create_texture(device: &IDirect3DDevice9, size: [usize; 2]) -> Result<IDirect3DTexture9> {
    let width = u32::try_from(size[0]).map_err(|_| Error::new("egui texture is too wide"))?;
    let height = u32::try_from(size[1]).map_err(|_| Error::new("egui texture is too tall"))?;
    let mut texture = None;
    unsafe {
        device.CreateTexture(
            width,
            height,
            1,
            D3DUSAGE_DYNAMIC as u32,
            D3DFMT_A8R8G8B8,
            D3DPOOL_DEFAULT,
            &mut texture,
            ptr::null_mut::<HANDLE>(),
        )
    }
    .map_err(|error| d3d_error("IDirect3DDevice9::CreateTexture", error))?;
    texture.ok_or_else(|| Error::new("D3D9 returned a null texture"))
}

fn upload_vertex_buffer(buffer: &IDirect3DVertexBuffer9, vertices: &[Vertex]) -> Result<()> {
    upload_buffer(
        vertices,
        |bytes, destination| unsafe { buffer.Lock(0, bytes, destination, D3DLOCK_DISCARD as u32) },
        || unsafe { buffer.Unlock() },
        "vertex buffer",
    )
}

fn upload_index_buffer(buffer: &IDirect3DIndexBuffer9, indices: &[u32]) -> Result<()> {
    upload_buffer(
        indices,
        |bytes, destination| unsafe { buffer.Lock(0, bytes, destination, D3DLOCK_DISCARD as u32) },
        || unsafe { buffer.Unlock() },
        "index buffer",
    )
}

fn upload_buffer<T: Copy>(
    values: &[T],
    lock: impl FnOnce(u32, *mut *mut c_void) -> windows::core::Result<()>,
    unlock: impl FnOnce() -> windows::core::Result<()>,
    name: &'static str,
) -> Result<()> {
    let bytes = byte_len::<T>(values.len(), name)?;
    let mut destination = ptr::null_mut();
    lock(bytes, &mut destination).map_err(|error| d3d_error("IDirect3DResource9::Lock", error))?;
    let copy_result = if destination.is_null() {
        Err(Error::new(format!("D3D9 returned a null {name} mapping")))
    } else {
        unsafe {
            ptr::copy_nonoverlapping(values.as_ptr(), destination.cast::<T>(), values.len());
        }
        Ok(())
    };
    let unlock_result = unlock().map_err(|error| d3d_error("IDirect3DResource9::Unlock", error));
    copy_result.and(unlock_result)
}

fn upload_texture(texture: &IDirect3DTexture9, size: [usize; 2], pixels: &[Bgra]) -> Result<()> {
    let expected_pixels = size[0]
        .checked_mul(size[1])
        .ok_or_else(|| Error::new("egui texture dimensions overflow"))?;
    if pixels.len() != expected_pixels {
        return Err(Error::new("egui texture has an invalid pixel count"));
    }

    let mut locked = D3DLOCKED_RECT::default();
    unsafe { texture.LockRect(0, &mut locked, ptr::null(), D3DLOCK_DISCARD as u32) }
        .map_err(|error| d3d_error("IDirect3DTexture9::LockRect", error))?;

    let row_bytes = size[0]
        .checked_mul(mem::size_of::<Bgra>())
        .ok_or_else(|| Error::new("egui texture row is too wide"))?;
    let pitch =
        usize::try_from(locked.Pitch).map_err(|_| Error::new("negative D3D9 texture pitch"));
    let copy_result = pitch.and_then(|pitch| {
        if locked.pBits.is_null() {
            return Err(Error::new("D3D9 returned a null texture mapping"));
        }
        if pitch < row_bytes {
            return Err(Error::new(
                "D3D9 texture pitch is smaller than one egui row",
            ));
        }
        for row in 0..size[1] {
            unsafe {
                ptr::copy_nonoverlapping(
                    pixels.as_ptr().add(row * size[0]).cast::<u8>(),
                    locked.pBits.cast::<u8>().add(row * pitch),
                    row_bytes,
                );
            }
        }
        Ok(())
    });
    let unlock_result = unsafe { texture.UnlockRect(0) }
        .map_err(|error| d3d_error("IDirect3DTexture9::UnlockRect", error));
    copy_result.and(unlock_result)
}

fn byte_len<T>(count: usize, resource: &'static str) -> Result<u32> {
    count
        .checked_mul(mem::size_of::<T>())
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(|| Error::new(format!("{resource} exceeds D3D9 size limits")))
}

fn grown_capacity(initial: usize, required: usize) -> Result<usize> {
    required
        .checked_next_power_of_two()
        .map(|required| initial.max(required))
        .ok_or_else(|| Error::new("egui mesh exceeds addressable buffer capacity"))
}

unsafe fn setup_render_state(
    device: &IDirect3DDevice9,
    screen_points: [f32; 2],
    screen_pixels: [u32; 2],
    pixels_per_point: f32,
) -> Result<()> {
    let identity = Matrix4x4 {
        M11: 1.0,
        M22: 1.0,
        M33: 1.0,
        M44: 1.0,
        ..Matrix4x4::default()
    };
    let half_pixel = 0.5 / pixels_per_point;
    let left = half_pixel;
    let right = screen_points[0] + half_pixel;
    let top = half_pixel;
    let bottom = screen_points[1] + half_pixel;
    let projection = Matrix4x4 {
        M11: 2.0 / (right - left),
        M22: 2.0 / (top - bottom),
        M33: 0.5,
        M41: (right + left) / (left - right),
        M42: (top + bottom) / (bottom - top),
        M43: 0.5,
        M44: 1.0,
        ..Matrix4x4::default()
    };

    unsafe {
        device
            .SetViewport(&D3DVIEWPORT9 {
                X: 0,
                Y: 0,
                Width: screen_pixels[0],
                Height: screen_pixels[1],
                MinZ: 0.0,
                MaxZ: 1.0,
            })
            .map_err(|error| d3d_error("IDirect3DDevice9::SetViewport", error))?;
        device
            .SetPixelShader(None)
            .map_err(|error| d3d_error("IDirect3DDevice9::SetPixelShader", error))?;
        device
            .SetVertexShader(None)
            .map_err(|error| d3d_error("IDirect3DDevice9::SetVertexShader", error))?;
        device
            .SetFVF(D3DFVF_EGUI_VERTEX)
            .map_err(|error| d3d_error("IDirect3DDevice9::SetFVF", error))?;
        device
            .SetTransform(D3DTS_WORLD_MATRIX, &identity)
            .map_err(|error| d3d_error("IDirect3DDevice9::SetTransform(world)", error))?;
        device
            .SetTransform(D3DTS_VIEW, &identity)
            .map_err(|error| d3d_error("IDirect3DDevice9::SetTransform(view)", error))?;
        device
            .SetTransform(D3DTS_PROJECTION, &projection)
            .map_err(|error| d3d_error("IDirect3DDevice9::SetTransform(projection)", error))?;

        for (state, value) in [
            (D3DRS_FILLMODE, D3DFILL_SOLID.0 as u32),
            (D3DRS_SHADEMODE, D3DSHADE_GOURAUD.0 as u32),
            (D3DRS_ZENABLE, 0),
            (D3DRS_ZWRITEENABLE, 0),
            (D3DRS_ALPHATESTENABLE, 0),
            (D3DRS_CULLMODE, D3DCULL_NONE.0 as u32),
            (D3DRS_ALPHABLENDENABLE, 1),
            (D3DRS_BLENDOP, D3DBLENDOP_ADD.0 as u32),
            (D3DRS_SRCBLEND, D3DBLEND_ONE.0 as u32),
            (D3DRS_DESTBLEND, D3DBLEND_INVSRCALPHA.0 as u32),
            (D3DRS_SEPARATEALPHABLENDENABLE, 1),
            (D3DRS_BLENDOPALPHA, D3DBLENDOP_ADD.0 as u32),
            (D3DRS_SRCBLENDALPHA, D3DBLEND_ONE.0 as u32),
            (D3DRS_DESTBLENDALPHA, D3DBLEND_INVSRCALPHA.0 as u32),
            (D3DRS_SCISSORTESTENABLE, 1),
            (D3DRS_FOGENABLE, 0),
            (D3DRS_STENCILENABLE, 0),
            (D3DRS_CLIPPING, 1),
            (D3DRS_LIGHTING, 0),
            (D3DRS_COLORWRITEENABLE, u32::MAX),
            (D3DRS_SRGBWRITEENABLE, 0),
        ] {
            set_render_state(device, state, value)?;
        }

        for (state, value) in [
            (D3DTSS_COLOROP, D3DTOP_MODULATE.0 as u32),
            (D3DTSS_COLORARG1, D3DTA_TEXTURE),
            (D3DTSS_COLORARG2, D3DTA_DIFFUSE),
            (D3DTSS_ALPHAOP, D3DTOP_MODULATE.0 as u32),
            (D3DTSS_ALPHAARG1, D3DTA_TEXTURE),
            (D3DTSS_ALPHAARG2, D3DTA_DIFFUSE),
        ] {
            set_texture_stage_state(device, state, value)?;
        }
        for state in [D3DTSS_COLOROP, D3DTSS_ALPHAOP] {
            device
                .SetTextureStageState(1, state, D3DTOP_DISABLE.0 as u32)
                .map_err(|error| d3d_error("IDirect3DDevice9::SetTextureStageState", error))?;
        }
    }

    Ok(())
}

unsafe fn set_sampler_state(
    device: &IDirect3DDevice9,
    options: egui::TextureOptions,
) -> Result<()> {
    let magnification = texture_filter(options.magnification);
    let minification = texture_filter(options.minification);
    let address = match options.wrap_mode {
        TextureWrapMode::ClampToEdge => D3DTADDRESS_CLAMP,
        TextureWrapMode::Repeat => D3DTADDRESS_WRAP,
        TextureWrapMode::MirroredRepeat => D3DTADDRESS_MIRROR,
    };
    unsafe {
        for (state, value) in [
            (D3DSAMP_MAGFILTER, magnification.0 as u32),
            (D3DSAMP_MINFILTER, minification.0 as u32),
            (D3DSAMP_MIPFILTER, D3DTEXF_NONE.0 as u32),
        ] {
            device
                .SetSamplerState(0, state, value)
                .map_err(|error| d3d_error("IDirect3DDevice9::SetSamplerState", error))?;
        }
        for state in [D3DSAMP_ADDRESSU, D3DSAMP_ADDRESSV, D3DSAMP_ADDRESSW] {
            device
                .SetSamplerState(0, state, address.0 as u32)
                .map_err(|error| d3d_error("IDirect3DDevice9::SetSamplerState", error))?;
        }
    }
    Ok(())
}

fn texture_filter(
    filter: TextureFilter,
) -> windows::Win32::Graphics::Direct3D9::D3DTEXTUREFILTERTYPE {
    match filter {
        TextureFilter::Nearest => D3DTEXF_POINT,
        TextureFilter::Linear => D3DTEXF_LINEAR,
    }
}

unsafe fn set_render_state(
    device: &IDirect3DDevice9,
    state: windows::Win32::Graphics::Direct3D9::D3DRENDERSTATETYPE,
    value: u32,
) -> Result<()> {
    unsafe { device.SetRenderState(state, value) }
        .map_err(|error| d3d_error("IDirect3DDevice9::SetRenderState", error))
}

unsafe fn set_texture_stage_state(
    device: &IDirect3DDevice9,
    state: windows::Win32::Graphics::Direct3D9::D3DTEXTURESTAGESTATETYPE,
    value: u32,
) -> Result<()> {
    unsafe { device.SetTextureStageState(0, state, value) }
        .map_err(|error| d3d_error("IDirect3DDevice9::SetTextureStageState", error))
}

struct StateBackup<'a> {
    device: &'a IDirect3DDevice9,
    state: IDirect3DStateBlock9,
    world: Matrix4x4,
    view: Matrix4x4,
    projection: Matrix4x4,
    viewport: D3DVIEWPORT9,
    render_target: IDirect3DSurface9,
    restored: bool,
}

impl<'a> StateBackup<'a> {
    fn capture(device: &'a IDirect3DDevice9) -> Result<Self> {
        unsafe {
            let state = device
                .CreateStateBlock(D3DSBT_ALL)
                .map_err(|error| d3d_error("IDirect3DDevice9::CreateStateBlock", error))?;
            state
                .Capture()
                .map_err(|error| d3d_error("IDirect3DStateBlock9::Capture", error))?;
            let mut world = Matrix4x4::default();
            let mut view = Matrix4x4::default();
            let mut projection = Matrix4x4::default();
            let mut viewport = D3DVIEWPORT9::default();
            device
                .GetTransform(D3DTS_WORLD_MATRIX, &mut world)
                .map_err(|error| d3d_error("IDirect3DDevice9::GetTransform(world)", error))?;
            device
                .GetTransform(D3DTS_VIEW, &mut view)
                .map_err(|error| d3d_error("IDirect3DDevice9::GetTransform(view)", error))?;
            device
                .GetTransform(D3DTS_PROJECTION, &mut projection)
                .map_err(|error| d3d_error("IDirect3DDevice9::GetTransform(projection)", error))?;
            device
                .GetViewport(&mut viewport)
                .map_err(|error| d3d_error("IDirect3DDevice9::GetViewport", error))?;
            let render_target = device
                .GetRenderTarget(0)
                .map_err(|error| d3d_error("IDirect3DDevice9::GetRenderTarget", error))?;
            Ok(Self {
                device,
                state,
                world,
                view,
                projection,
                viewport,
                render_target,
                restored: false,
            })
        }
    }

    fn restore(&mut self) -> Result<()> {
        let result = (|| unsafe {
            self.state
                .Apply()
                .map_err(|error| d3d_error("IDirect3DStateBlock9::Apply", error))?;
            self.device
                .SetTransform(D3DTS_WORLD_MATRIX, &self.world)
                .map_err(|error| d3d_error("IDirect3DDevice9::SetTransform(world)", error))?;
            self.device
                .SetTransform(D3DTS_VIEW, &self.view)
                .map_err(|error| d3d_error("IDirect3DDevice9::SetTransform(view)", error))?;
            self.device
                .SetTransform(D3DTS_PROJECTION, &self.projection)
                .map_err(|error| d3d_error("IDirect3DDevice9::SetTransform(projection)", error))?;
            self.device
                .SetViewport(&self.viewport)
                .map_err(|error| d3d_error("IDirect3DDevice9::SetViewport", error))?;
            self.device
                .SetRenderTarget(0, &self.render_target)
                .map_err(|error| d3d_error("IDirect3DDevice9::SetRenderTarget", error))
        })();
        self.restored = true;
        result
    }
}

impl Drop for StateBackup<'_> {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore();
        }
    }
}

fn d3d_error(operation: &'static str, error: windows::core::Error) -> Error {
    Error::new(format!("{operation} failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_premultiplied_rgba_to_d3d_bgra() {
        let pixel = Bgra::from(Color32::from_rgba_premultiplied(10, 20, 30, 40));
        assert_eq!(
            [pixel.blue, pixel.green, pixel.red, pixel.alpha],
            [30, 20, 10, 40]
        );
    }

    #[test]
    fn clips_scissors_to_the_backbuffer() {
        let rect = clip_rect(
            egui::Rect::from_min_max(egui::pos2(-2.0, 3.0), egui::pos2(20.2, 30.8)),
            2.0,
            [32, 48],
        )
        .unwrap();
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (0, 6, 32, 48)
        );
    }

    #[test]
    fn patches_texture_rows_without_touching_neighbors() {
        let mut texture = Texture {
            handle: None,
            size: [3, 2],
            pixels: vec![Bgra::from(Color32::BLACK); 6],
            options: egui::TextureOptions::LINEAR,
        };
        let white = Bgra::from(Color32::WHITE);
        texture.patch([1, 0], [1, 2], &[white, white]).unwrap();
        assert_eq!(texture.pixels[0].red, 0);
        assert_eq!(texture.pixels[1].red, 255);
        assert_eq!(texture.pixels[4].red, 255);
        assert_eq!(texture.pixels[5].red, 0);
    }
}
