use crate::{
    guides,
    preview::{Camera, PreviewOptions},
};
use windows::Win32::Graphics::Direct3D9::*;

/// Draw depth-tested guides after native assets, restoring the device state so
/// native state caches continue to agree with the actual device. No GPU resource
/// is retained across frames or device resets.
pub(super) unsafe fn draw(
    device: &IDirect3DDevice9,
    camera: Camera,
    options: PreviewOptions,
) -> windows::core::Result<()> {
    if !options.show_grid && !options.show_axes {
        return Ok(());
    }
    let vertices = guides::vertices(camera, options);
    unsafe {
        let state = device.CreateStateBlock(D3DSBT_ALL)?;
        state.Capture()?;
        let transforms = [D3DTRANSFORMSTATETYPE(256), D3DTS_VIEW, D3DTS_PROJECTION];
        let mut previous = [Default::default(); 3];
        for (transform, matrix) in transforms.into_iter().zip(&mut previous) {
            device.GetTransform(transform, matrix)?;
        }
        let (view, projection) = camera.matrices(1.0, 200_000.0);
        let identity: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let draw = (|| -> windows::core::Result<()> {
            device.SetPixelShader(None)?;
            device.SetVertexShader(None)?;
            device.SetFVF(D3DFVF_XYZ | D3DFVF_DIFFUSE)?;
            for (transform, matrix) in transforms.into_iter().zip([identity, view, projection]) {
                // Matrix4x4 uses the same contiguous row-major 16-float layout.
                device.SetTransform(transform, matrix.as_ptr().cast())?;
            }
            for (state, value) in [
                (D3DRS_ZENABLE, 1),
                (D3DRS_ZWRITEENABLE, 0),
                (D3DRS_ZFUNC, D3DCMP_LESSEQUAL.0 as u32),
                (D3DRS_LIGHTING, 0),
                (D3DRS_FOGENABLE, 0),
                (D3DRS_ALPHATESTENABLE, 0),
                (D3DRS_ALPHABLENDENABLE, 0),
                (D3DRS_STENCILENABLE, 0),
                (D3DRS_SCISSORTESTENABLE, 0),
                (D3DRS_CLIPPING, 1),
                (D3DRS_CLIPPLANEENABLE, 0),
                (D3DRS_VERTEXBLEND, 0),
                (D3DRS_INDEXEDVERTEXBLENDENABLE, 0),
                (D3DRS_COLORWRITEENABLE, 15),
                (D3DRS_SRGBWRITEENABLE, 0),
                (D3DRS_DEPTHBIAS, 0),
                (D3DRS_SLOPESCALEDEPTHBIAS, 0),
            ] {
                device.SetRenderState(state, value)?;
            }
            device.SetTexture(0, None)?;
            for (state, value) in [
                (D3DTSS_COLOROP, D3DTOP_SELECTARG1.0 as u32),
                (D3DTSS_COLORARG1, D3DTA_DIFFUSE),
                (D3DTSS_ALPHAOP, D3DTOP_SELECTARG1.0 as u32),
                (D3DTSS_ALPHAARG1, D3DTA_DIFFUSE),
            ] {
                device.SetTextureStageState(0, state, value)?;
            }
            device.SetTextureStageState(1, D3DTSS_COLOROP, D3DTOP_DISABLE.0 as u32)?;
            device.SetTextureStageState(1, D3DTSS_ALPHAOP, D3DTOP_DISABLE.0 as u32)?;
            device.DrawPrimitiveUP(
                D3DPT_LINELIST,
                (vertices.len() / 2) as u32,
                vertices.as_ptr().cast(),
                std::mem::size_of::<guides::Vertex>() as u32,
            )
        })();
        // Restore even if setup/drawing failed. Some D3D9 implementations
        // need explicit transform restoration in addition to the state block.
        let mut restore = state.Apply();
        for (transform, matrix) in transforms.into_iter().zip(&previous) {
            restore = restore.and(device.SetTransform(transform, matrix));
        }
        draw.and(restore)
    }
}
