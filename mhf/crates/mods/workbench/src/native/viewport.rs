use super::{Client, get, put};
use crate::preview::{Viewport, ViewportPixels};
use std::ptr;
use windows::Win32::Graphics::Direct3D9::{
    D3DCLEAR_TARGET, D3DCLEAR_ZBUFFER, D3DSURFACE_DESC, D3DVIEWPORT9, IDirect3DDevice9,
};

const NATIVE_ASPECT: usize = 0x119d_d768;

/// Applies a viewport only inside the queued world-render callback. Native
/// 10007F50 -> 10013440 -> 10018D30 submits DrawIndexedPrimitive synchronously
/// (for example 10018F60/100190B8), so restoring after asset.draw returns does
/// not move the workbench's draws into the next caller's viewport.
pub(super) struct NativeViewport<'a> {
    device: &'a IDirect3DDevice9,
    previous: D3DVIEWPORT9,
    aspect_address: usize,
    previous_aspect: u32,
    restored: bool,
    pub pixels: Option<ViewportPixels>,
    pub normalized: Viewport,
}

impl<'a> NativeViewport<'a> {
    /// # Safety
    /// The client is retained and this is the native world-render thread.
    pub unsafe fn begin(
        device: &'a IDirect3DDevice9,
        client: Client,
        requested: Viewport,
    ) -> Result<Self, String> {
        let mut previous = D3DVIEWPORT9::default();
        unsafe { device.GetViewport(&mut previous) }
            .map_err(|error| format!("无法读取原生视口：{error}"))?;
        let target = unsafe { device.GetRenderTarget(0) }
            .map_err(|error| format!("无法读取原生绘制目标：{error}"))?;
        let mut description = D3DSURFACE_DESC::default();
        unsafe { target.GetDesc(&mut description) }
            .map_err(|error| format!("无法读取原生绘制目标尺寸：{error}"))?;
        if description.Width == 0 || description.Height == 0 {
            return Err("原生绘制目标尺寸为空".into());
        }
        let pixels = requested.pixels(description.Width, description.Height);
        let aspect_address = client.address(NATIVE_ASPECT);
        let scope = Self {
            device,
            previous,
            aspect_address,
            // Preserve the original bits, even if a reset left a non-finite value.
            previous_aspect: unsafe { get(aspect_address) },
            restored: false,
            pixels,
            normalized: pixels.map_or_else(
                || requested.clipped(),
                |pixels| pixels.normalized(description.Width, description.Height),
            ),
        };
        // Clear the complete workbench background before narrowing the viewport,
        // including pixels exposed when a sidebar moves or compact mode opens.
        unsafe {
            device.SetViewport(&D3DVIEWPORT9 {
                X: 0,
                Y: 0,
                Width: description.Width,
                Height: description.Height,
                MinZ: 0.0,
                MaxZ: 1.0,
            })
        }
        .map_err(|error| format!("无法设置清理视口：{error}"))?;
        unsafe {
            device.Clear(
                0,
                ptr::null(),
                (D3DCLEAR_TARGET | D3DCLEAR_ZBUFFER) as u32,
                0xff101316,
                1.0,
                0,
            )
        }
        .map_err(|error| format!("无法清理预览画面：{error}"))?;
        if let Some(pixels) = pixels {
            unsafe {
                device.SetViewport(&D3DVIEWPORT9 {
                    X: pixels.x,
                    Y: pixels.y,
                    Width: pixels.width,
                    Height: pixels.height,
                    MinZ: 0.0,
                    MaxZ: 1.0,
                })
            }
            .map_err(|error| format!("无法设置中央视口：{error}"))?;
            // 10006B10 reads this global for culling. Keep it consistent with
            // camera projection throughout this draw, and restore it on exit.
            unsafe { put(aspect_address, pixels.aspect()) };
        }
        Ok(scope)
    }

    pub fn restore(&mut self) -> Result<(), String> {
        unsafe {
            put(self.aspect_address, self.previous_aspect);
            self.device.SetViewport(&self.previous)
        }
        .map_err(|error| format!("无法恢复原生视口：{error}"))?;
        self.restored = true;
        Ok(())
    }
}

impl Drop for NativeViewport<'_> {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.restore();
        }
    }
}
