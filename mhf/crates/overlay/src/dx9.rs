//! Direct3D 9 hook installation and lifecycle.

use std::any::Any;
use std::ffi::c_void;
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mhf_hooks::{HookGuard, HookSlot};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D9::{
    D3D_SDK_VERSION, D3DADAPTER_DEFAULT, D3DCREATE_SOFTWARE_VERTEXPROCESSING,
    D3DDEVICE_CREATION_PARAMETERS, D3DDEVTYPE_NULLREF, D3DDISPLAYMODE, D3DPRESENT_PARAMETERS,
    D3DSWAPEFFECT_DISCARD, Direct3DCreate9, IDirect3DDevice9,
};
use windows::Win32::Graphics::Gdi::RGNDATA;
use windows::core::{BOOL, HRESULT, Interface};

use crate::renderer::Renderer;
use crate::window::{DummyWindow, WindowBinding, WindowState};
use crate::{Error, HostIme, InputCaptureState, Overlay, Result};

type Present = unsafe extern "system" fn(
    device: *mut c_void,
    source_rect: *const RECT,
    destination_rect: *const RECT,
    destination_window: HWND,
    dirty_region: *const RGNDATA,
) -> HRESULT;
type Reset = unsafe extern "system" fn(
    device: *mut c_void,
    presentation_parameters: *mut D3DPRESENT_PARAMETERS,
) -> HRESULT;

static PRESENT_TARGET: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static RESET_TARGET: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static HOOK_STATE: HookSlot<HookState> = HookSlot::new();
static LAST_ERROR: Mutex<Option<Error>> = Mutex::new(None);

const D3DERR_INVALIDCALL: HRESULT = HRESULT(0x8876_086C_u32.cast_signed());

/// Owns the process-local D3D9 hooks.
///
/// Dropping the handle disables and removes only the `Present` and `Reset`
/// hooks installed by this instance. Other `MinHook` users remain enabled.
#[must_use = "dropping the handle removes the D3D9 overlay hooks"]
pub struct D3d9Hook {
    hooks: HookGuard<HookState>,
    capture: InputCaptureState,
}

impl D3d9Hook {
    /// Resolves the D3D9 device vtable and installs the overlay hooks.
    ///
    /// # Errors
    ///
    /// Returns an error if another overlay is active or a D3D9/`MinHook`
    /// operation fails.
    ///
    /// # Safety
    ///
    /// This patches process-wide executable code. The caller must ensure the
    /// host process uses a compatible Direct3D 9 runtime, keep the returned
    /// handle alive for as long as hooked frames may execute, and uninstall it
    /// on a thread allowed to release the device resources. If the host invokes
    /// the device from multiple threads, it must enable D3D9 multithreaded mode.
    /// Uninstall on the focus-window thread, or keep that thread pumping messages
    /// until uninstall returns, so its IME context can be restored synchronously.
    pub unsafe fn install(overlay: impl Overlay) -> Result<Self> {
        unsafe { Self::install_inner(overlay, None) }
    }

    /// Installs the overlay with a native editor sharing its IME context.
    ///
    /// # Safety
    ///
    /// The caller must satisfy the same requirements as [`Self::install`]. The
    /// host adapter must remain valid until synchronous overlay cleanup finishes.
    pub unsafe fn install_with_ime(overlay: impl Overlay, ime: Arc<dyn HostIme>) -> Result<Self> {
        unsafe { Self::install_inner(overlay, Some(ime)) }
    }

    unsafe fn install_inner(overlay: impl Overlay, ime: Option<Arc<dyn HostIme>>) -> Result<Self> {
        let mut hooks = HOOK_STATE.prepare().map_err(Error::new)?;
        *last_error() = None;

        let targets = resolve_targets()?;
        let present_original = unsafe {
            hooks.create(
                "D3D9 Present",
                targets.present as *mut c_void,
                present_hook as Present as *mut c_void,
            )
        }
        .map_err(Error::new)?;
        let reset_original = unsafe {
            hooks.create(
                "D3D9 Reset",
                targets.reset as *mut c_void,
                reset_hook as Reset as *mut c_void,
            )
        }
        .map_err(Error::new)?;

        PRESENT_TARGET.store(targets.present as *mut c_void, Ordering::Release);
        RESET_TARGET.store(targets.reset as *mut c_void, Ordering::Release);
        let capture = InputCaptureState::default();
        let state = HookState {
            present: unsafe { mem::transmute::<*mut c_void, Present>(present_original) },
            reset: unsafe { mem::transmute::<*mut c_void, Reset>(reset_original) },
            runtime: Mutex::new(Runtime::new(Box::new(overlay), capture.clone(), ime)),
        };
        let hooks = unsafe { hooks.install(state) }.map_err(Error::new)?;
        Ok(Self { hooks, capture })
    }

    /// Shares the caller's capture decisions with a game-specific input adapter.
    pub fn input_capture(&self) -> InputCaptureState {
        self.capture.clone()
    }

    /// Returns and clears the most recent render or initialization error.
    pub fn take_render_error(&self) -> Option<Error> {
        last_error().take()
    }

    /// Removes the hooks immediately instead of waiting for `Drop`.
    ///
    /// # Errors
    ///
    /// Returns an error if either hook cannot be disabled or removed.
    pub fn uninstall(mut self) -> Result<()> {
        self.capture.reset();
        self.hooks.uninstall().map_err(Error::new)
    }
}

impl Drop for D3d9Hook {
    fn drop(&mut self) {
        self.capture.reset();
    }
}

struct Targets {
    present: usize,
    reset: usize,
}

struct HookState {
    present: Present,
    reset: Reset,
    runtime: Mutex<Runtime>,
}

impl HookState {
    fn runtime(&self) -> MutexGuard<'_, Runtime> {
        self.runtime.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Runtime {
    overlay: Option<Box<dyn Overlay>>,
    pipeline: Option<Pipeline>,
    rendering_disabled: bool,
    capture: InputCaptureState,
    ime: Option<Arc<dyn HostIme>>,
}

impl Runtime {
    fn new(
        overlay: Box<dyn Overlay>,
        capture: InputCaptureState,
        ime: Option<Arc<dyn HostIme>>,
    ) -> Self {
        Self {
            overlay: Some(overlay),
            pipeline: None,
            rendering_disabled: false,
            capture,
            ime,
        }
    }

    fn render(&mut self, device: &IDirect3DDevice9) {
        if self.rendering_disabled {
            return;
        }

        let render_result = catch_unwind(AssertUnwindSafe(|| -> Result<()> {
            if self.pipeline.is_none() {
                self.initialize_pipeline(device)?;
            }
            let pipeline = self
                .pipeline
                .as_mut()
                .expect("pipeline must exist after initialization");
            if pipeline.matches(device) {
                pipeline.frame(device)?;
            }
            Ok(())
        }));

        match render_result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                self.clear_capture();
                *last_error() = Some(error);
            }
            Err(payload) => {
                self.clear_capture();
                *last_error() = Some(Error::new(format!(
                    "overlay render loop panicked: {}",
                    panic_message(payload.as_ref())
                )));
                self.rendering_disabled = true;
            }
        }
    }

    fn clear_capture(&self) {
        if let Some(pipeline) = &self.pipeline {
            pipeline.window_state.clear_capture();
        }
    }

    fn initialize_pipeline(&mut self, device: &IDirect3DDevice9) -> Result<()> {
        let hwnd = device_window(device)?;
        let window_state = Arc::new(WindowState::new(
            hwnd,
            self.capture.clone(),
            self.ime.clone(),
        ));
        let window = WindowBinding::install(hwnd, Arc::clone(&window_state))?;
        let context = egui::Context::default();
        self.overlay
            .as_mut()
            .expect("overlay must exist before pipeline initialization")
            .initialize(&context);
        let overlay = self
            .overlay
            .take()
            .expect("overlay must exist before pipeline initialization");
        self.pipeline = Some(Pipeline {
            device: Interface::as_raw(device) as usize,
            context,
            window_state,
            renderer: Renderer::default(),
            overlay,
            _window: window,
        });
        Ok(())
    }

    fn prepare_for_reset(&mut self, device: &IDirect3DDevice9) {
        if let Some(pipeline) = self
            .pipeline
            .as_mut()
            .filter(|pipeline| pipeline.matches(device))
        {
            pipeline.window_state.clear_capture();
            pipeline.renderer.prepare_for_reset();
        }
    }
}

struct Pipeline {
    device: usize,
    context: egui::Context,
    window_state: Arc<WindowState>,
    renderer: Renderer,
    overlay: Box<dyn Overlay>,
    _window: WindowBinding,
}

// SAFETY: `install` requires the host device to support every thread that can
// enter these hooks or uninstall them, and the runtime mutex serializes access.
unsafe impl Send for Pipeline {}

impl Pipeline {
    fn matches(&self, device: &IDirect3DDevice9) -> bool {
        self.device == Interface::as_raw(device) as usize
    }

    fn frame(&mut self, device: &IDirect3DDevice9) -> Result<()> {
        if unsafe { device.TestCooperativeLevel() }.is_err() {
            self.window_state.clear_capture();
            return Ok(());
        }

        let pixels_per_point = self.context.pixels_per_point();
        let input = self.window_state.take_input(pixels_per_point)?;
        let output = self.context.run_ui(input, |ui| self.overlay.ui(ui));
        let mut textures_delta = TextureDeltaGuard(output.textures_delta);
        self.window_state
            .update_capture(self.overlay.input_policy(&self.context), &self.context);
        self.window_state
            .update_ime(&self.context, &output.platform_output);
        self.overlay
            .platform_output(&self.context, &output.platform_output);

        let primitives = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        self.renderer.paint(
            device,
            primitives,
            &mut textures_delta.0,
            output.pixels_per_point,
        )
    }
}

struct TextureDeltaGuard(egui::TexturesDelta);

impl Drop for TextureDeltaGuard {
    fn drop(&mut self) {
        self.0.clear();
    }
}

unsafe extern "system" fn present_hook(
    device_raw: *mut c_void,
    source_rect: *const RECT,
    destination_rect: *const RECT,
    destination_window: HWND,
    dirty_region: *const RGNDATA,
) -> HRESULT {
    let invocation = HOOK_STATE.enter();
    let state = invocation.state();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // D3D9 passes a live, borrowed COM `this` pointer to every vtable call.
        let Some(device) = (unsafe { IDirect3DDevice9::from_raw_borrowed(&device_raw) }) else {
            return;
        };
        if let Some(state) = state {
            state.runtime().render(device);
        }
    }));

    let original = match state {
        Some(state) => state.present,
        None => {
            let target = PRESENT_TARGET.load(Ordering::Acquire);
            if target.is_null() {
                return D3DERR_INVALIDCALL;
            }
            unsafe { mem::transmute::<*mut c_void, Present>(target) }
        }
    };
    unsafe {
        original(
            device_raw,
            source_rect,
            destination_rect,
            destination_window,
            dirty_region,
        )
    }
}

unsafe extern "system" fn reset_hook(
    device_raw: *mut c_void,
    presentation_parameters: *mut D3DPRESENT_PARAMETERS,
) -> HRESULT {
    let invocation = HOOK_STATE.enter();
    let state = invocation.state();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // D3D9 passes a live, borrowed COM `this` pointer to every vtable call.
        let Some(device) = (unsafe { IDirect3DDevice9::from_raw_borrowed(&device_raw) }) else {
            return;
        };
        if let Some(state) = state {
            state.runtime().prepare_for_reset(device);
        }
    }));

    let original = match state {
        Some(state) => state.reset,
        None => {
            let target = RESET_TARGET.load(Ordering::Acquire);
            if target.is_null() {
                return D3DERR_INVALIDCALL;
            }
            unsafe { mem::transmute::<*mut c_void, Reset>(target) }
        }
    };
    unsafe { original(device_raw, presentation_parameters) }
}

fn resolve_targets() -> Result<Targets> {
    let window = DummyWindow::create()?;
    let direct3d = unsafe { Direct3DCreate9(D3D_SDK_VERSION) }
        .ok_or_else(|| Error::new("Direct3DCreate9 returned null"))?;
    let mut display_mode = D3DDISPLAYMODE::default();
    unsafe { direct3d.GetAdapterDisplayMode(D3DADAPTER_DEFAULT, &mut display_mode) }.map_err(
        |error| Error::new(format!("IDirect3D9::GetAdapterDisplayMode failed: {error}")),
    )?;
    let mut parameters = D3DPRESENT_PARAMETERS {
        BackBufferWidth: 1,
        BackBufferHeight: 1,
        BackBufferFormat: display_mode.Format,
        BackBufferCount: 1,
        SwapEffect: D3DSWAPEFFECT_DISCARD,
        hDeviceWindow: window.hwnd(),
        Windowed: BOOL(1),
        ..Default::default()
    };
    let mut device = None;
    unsafe {
        direct3d.CreateDevice(
            D3DADAPTER_DEFAULT,
            D3DDEVTYPE_NULLREF,
            window.hwnd(),
            D3DCREATE_SOFTWARE_VERTEXPROCESSING as u32,
            &mut parameters,
            &mut device,
        )
    }
    .map_err(|error| Error::new(format!("IDirect3D9::CreateDevice failed: {error}")))?;
    let device = device.ok_or_else(|| Error::new("D3D9 returned a null discovery device"))?;

    Ok(Targets {
        present: device.vtable().Present as usize,
        reset: device.vtable().Reset as usize,
    })
}

fn device_window(device: &IDirect3DDevice9) -> Result<HWND> {
    let mut creation = D3DDEVICE_CREATION_PARAMETERS::default();
    unsafe { device.GetCreationParameters(&mut creation) }.map_err(|error| {
        Error::new(format!(
            "IDirect3DDevice9::GetCreationParameters failed: {error}"
        ))
    })?;
    if !creation.hFocusWindow.is_invalid() {
        return Ok(creation.hFocusWindow);
    }

    let swap_chain = unsafe { device.GetSwapChain(0) }
        .map_err(|error| Error::new(format!("IDirect3DDevice9::GetSwapChain failed: {error}")))?;
    let mut parameters = D3DPRESENT_PARAMETERS::default();
    unsafe { swap_chain.GetPresentParameters(&mut parameters) }.map_err(|error| {
        Error::new(format!(
            "IDirect3DSwapChain9::GetPresentParameters failed: {error}"
        ))
    })?;
    if parameters.hDeviceWindow.is_invalid() {
        Err(Error::new("D3D9 device and swap chain have no window"))
    } else {
        Ok(parameters.hDeviceWindow)
    }
}

fn last_error() -> MutexGuard<'static, Option<Error>> {
    LAST_ERROR.lock().unwrap_or_else(PoisonError::into_inner)
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}
