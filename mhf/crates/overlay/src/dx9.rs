//! Direct3D 9 hook installation and lifecycle.

use std::any::Any;
use std::ffi::c_void;
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use minhook::{MH_STATUS, MinHook};
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
use crate::{Error, Overlay, Result};

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
const PRESENT_DETOUR: Present = present_hook;
const RESET_DETOUR: Reset = reset_hook;

static PRESENT_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static RESET_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);
static LAST_ERROR: Mutex<Option<Error>> = Mutex::new(None);
static HOOK_BARRIER: RwLock<()> = RwLock::new(());

const D3DERR_INVALIDCALL: HRESULT = HRESULT(0x8876_086C_u32.cast_signed());

/// Owns the process-local D3D9 hooks.
///
/// Dropping the handle disables and removes only the `Present` and `Reset`
/// hooks installed by this instance. Other `MinHook` users remain enabled.
#[must_use = "dropping the handle removes the D3D9 overlay hooks"]
pub struct D3d9Hook {
    targets: Targets,
    active: bool,
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
    pub unsafe fn install(overlay: impl Overlay) -> Result<Self> {
        let mut runtime = runtime();
        if runtime.is_some() {
            return Err(Error::new("a D3D9 overlay hook is already installed"));
        }
        *last_error() = None;

        let targets = resolve_targets()?;
        let present_original = unsafe {
            MinHook::create_hook(
                targets.present as *mut c_void,
                PRESENT_DETOUR as *const () as *mut c_void,
            )
        }
        .map_err(|status| minhook_error("create D3D9 Present hook", status))?;
        let reset_original = match unsafe {
            MinHook::create_hook(
                targets.reset as *mut c_void,
                RESET_DETOUR as *const () as *mut c_void,
            )
        } {
            Ok(original) => original,
            Err(status) => {
                let error = minhook_error("create D3D9 Reset hook", status);
                return Err(with_cleanup_error(error, remove_hooks(targets)));
            }
        };

        PRESENT_ORIGINAL.store(present_original, Ordering::Release);
        RESET_ORIGINAL.store(reset_original, Ordering::Release);
        *runtime = Some(Runtime::new(Box::new(overlay)));

        for (name, target) in [("Present", targets.present), ("Reset", targets.reset)] {
            if let Err(status) = unsafe { MinHook::enable_hook(target as *mut c_void) } {
                let error = minhook_error(&format!("enable D3D9 {name} hook"), status);
                drop(runtime);
                return Err(with_cleanup_error(error, uninstall(targets)));
            }
        }

        Ok(Self {
            targets,
            active: true,
        })
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
        let result = uninstall(self.targets);
        if result.is_ok() {
            self.active = false;
        }
        result
    }
}

impl Drop for D3d9Hook {
    fn drop(&mut self) {
        if self.active {
            let _ = uninstall(self.targets);
            self.active = false;
        }
    }
}

#[derive(Clone, Copy)]
struct Targets {
    present: usize,
    reset: usize,
}

struct Runtime {
    overlay: Option<Box<dyn Overlay>>,
    pipeline: Option<Pipeline>,
    rendering_disabled: bool,
}

impl Runtime {
    fn new(overlay: Box<dyn Overlay>) -> Self {
        Self {
            overlay: Some(overlay),
            pipeline: None,
            rendering_disabled: false,
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
        let window_state = Arc::new(WindowState::new());
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
            hwnd: hwnd.0 as usize,
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
    hwnd: usize,
    context: egui::Context,
    window_state: Arc<WindowState>,
    renderer: Renderer,
    overlay: Box<dyn Overlay>,
    _window: WindowBinding,
}

// SAFETY: `install` requires the host device to support every thread that can
// enter these hooks or uninstall them, and `RUNTIME` serializes all access.
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
        let input = self
            .window_state
            .take_input(HWND(self.hwnd as *mut _), pixels_per_point)?;
        let output = self.context.run_ui(input, |ui| self.overlay.ui(ui.ctx()));
        let mut textures_delta = TextureDeltaGuard(output.textures_delta);
        self.window_state.update_capture(
            self.context.egui_wants_pointer_input(),
            self.context.egui_wants_keyboard_input(),
        );
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
    let _hook_guard = HOOK_BARRIER.read().unwrap_or_else(PoisonError::into_inner);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // D3D9 passes a live, borrowed COM `this` pointer to every vtable call.
        let Some(device) = (unsafe { IDirect3DDevice9::from_raw_borrowed(&device_raw) }) else {
            return;
        };
        if let Some(runtime) = runtime().as_mut() {
            runtime.render(device);
        }
    }));

    let original = PRESENT_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return D3DERR_INVALIDCALL;
    }
    let original = unsafe { mem::transmute::<*mut c_void, Present>(original) };
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
    let _hook_guard = HOOK_BARRIER.read().unwrap_or_else(PoisonError::into_inner);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // D3D9 passes a live, borrowed COM `this` pointer to every vtable call.
        let Some(device) = (unsafe { IDirect3DDevice9::from_raw_borrowed(&device_raw) }) else {
            return;
        };
        if let Some(runtime) = runtime().as_mut() {
            runtime.prepare_for_reset(device);
        }
    }));

    let original = RESET_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return D3DERR_INVALIDCALL;
    }
    let original = unsafe { mem::transmute::<*mut c_void, Reset>(original) };
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

fn uninstall(targets: Targets) -> Result<()> {
    for (name, target) in [("Present", targets.present), ("Reset", targets.reset)] {
        match unsafe { MinHook::disable_hook(target as *mut c_void) } {
            Err(status)
                if !matches!(
                    status,
                    MH_STATUS::MH_ERROR_DISABLED | MH_STATUS::MH_ERROR_NOT_CREATED
                ) =>
            {
                return Err(minhook_error(&format!("disable {name} hook"), status));
            }
            _ => {}
        }
    }

    let _hook_guard = HOOK_BARRIER.write().unwrap_or_else(PoisonError::into_inner);
    *runtime() = None;

    let result = remove_hooks(targets);
    if result.is_ok() {
        PRESENT_ORIGINAL.store(ptr::null_mut(), Ordering::Release);
        RESET_ORIGINAL.store(ptr::null_mut(), Ordering::Release);
    }
    result
}

fn remove_hooks(targets: Targets) -> Result<()> {
    let mut first_error = None;
    for (name, target) in [("Present", targets.present), ("Reset", targets.reset)] {
        match unsafe { MinHook::remove_hook(target as *mut c_void) } {
            Err(status) if status != MH_STATUS::MH_ERROR_NOT_CREATED => {
                first_error
                    .get_or_insert_with(|| minhook_error(&format!("remove {name} hook"), status));
            }
            _ => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn runtime() -> MutexGuard<'static, Option<Runtime>> {
    RUNTIME.lock().unwrap_or_else(PoisonError::into_inner)
}

fn last_error() -> MutexGuard<'static, Option<Error>> {
    LAST_ERROR.lock().unwrap_or_else(PoisonError::into_inner)
}

fn with_cleanup_error(error: Error, cleanup: Result<()>) -> Error {
    match cleanup {
        Ok(()) => error,
        Err(cleanup) => Error::new(format!("{error}; cleanup also failed: {cleanup}")),
    }
}

fn minhook_error(operation: &str, status: MH_STATUS) -> Error {
    Error::new(format!("failed to {operation}: {status:?}"))
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
