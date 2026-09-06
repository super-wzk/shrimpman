# MHF overlay

`mhf-overlay` installs process-local Direct3D 9 `Present` and `Reset`
hooks and renders an `egui` interface into the hooked device. It is intended
to be linked into a Windows process that already loads the target game DLL;
DLL injection is deliberately outside this crate.

Overlay callbacks run synchronously on the game's D3D9 rendering thread, so
networking, packet decoding, and other blocking work should feed the UI through
a channel or snapshot owned by the caller.

The hook handle owns only the two D3D9 hooks it installs. Dropping it restores
the original window procedure and removes those hooks without disabling other
`MinHook` users in the process.

Frame errors are available through `D3d9Hook::take_render_error` and are retried
on later frames. A panic disables overlay rendering; either failure path releases
input capture so the host window remains usable.

```rust,no_run
use mhf_overlay::{Overlay, dx9::D3d9Hook, egui};

struct DebugUi;

impl Overlay for DebugUi {
    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Window::new("Packet debugger").show(ui.ctx(), |ui| {
            ui.label("Ready");
        });
    }
}

fn install() -> Result<D3d9Hook, mhf_overlay::Error> {
    unsafe { D3d9Hook::install(DebugUi) }
}
```

## Caller-controlled input capture

Override `Overlay::input_policy` to choose mouse and keyboard behavior independently:

```rust
use mhf_overlay::{InputCapture, InputPolicy, Overlay, egui};

struct GameUi {
    menu_open: bool,
}

impl Overlay for GameUi {
    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Window::new("Menu").open(&mut self.menu_open).show(ui.ctx(), |ui| {
            ui.label("Ready");
        });
    }

    fn input_policy(&self, _: &egui::Context) -> InputPolicy {
        InputPolicy {
            pointer: InputCapture::Auto,
            keyboard: if self.menu_open {
                InputCapture::Block
            } else {
                InputCapture::PassThrough
            },
        }
    }
}
```

`Auto` follows egui's hover/drag or keyboard-focus intent, `Block` withholds the
channel from the game, and `PassThrough` forwards it even when egui wants input.
Both channels default to `Auto`. Egui receives the input in every mode; these
settings govern forwarding to the game's original window procedure.

The caller selects a policy from its active window/page/modal state. It is
sampled after each UI frame and applies to new presses. Press, repeat, drag and
release keep their original recipient even if a modal opens or a window closes
mid-gesture. Window focus loss clears that ownership; render errors release
capture for new input while preserving matching releases for held keys/buttons.

`D3d9Hook::input_capture()` returns a shared `InputCaptureState` for device input
adapters. `captures_pointer()` tests the current native cursor against the latest
interactive windows/areas, so a move and click between UI frames use the new
position. Click-through areas remain excluded. `captures_keyboard()` reports the
current keyboard decision. `pointer_button_owner()` and `scan_code_owner()` let
an adapter honor window presses that precede its first device sample; keyboard
ownership uses DirectInput scan codes, including extended keys. The adapter must
then keep each sampled press's owner until physical release.

`Overlay::ui` receives the native root `Ui`, so layouts, grids and panels can be
composed directly. Automatic pointer capture covers interactive floating windows
and areas; the root background remains the game. A page with interactive controls
mounted directly on that background should select `InputCapture::Block` while
active. Passive HUD content can keep `PassThrough`.

The overlay uses one egui context inside the game's existing D3D9 window. Egui
owns widget focus, layout, window ordering and paint output. Its default embedded
viewport mode also renders child viewports as egui windows. Creating additional
OS windows requires a host backend that implements the viewport lifecycle; this
backend does not create them. The `logic`/`ui` lifecycle of `eframe::App` belongs
to eframe's host loop and is not required to build UI in the D3D9 callback.

Capture is cleared on rendering failure, device reset, focus loss and hook
removal. A cloned capture handle can outlive the renderer without keeping input
blocked. The policy and `Overlay` interface also compile on non-Windows hosts
for testing; the D3D9 backend is Windows-only.

## Upstream references

The hook discovery and lifecycle follow the design of
[`hudhook`](https://github.com/veeenu/hudhook). The renderer and input boundary
were written against the current workspace `egui` version, using
[`egui-d3d9`](https://github.com/unknowntrojan/egui-d3d9) as a behavioral
reference. Both upstream projects are MIT licensed; their license notices are
recorded in `THIRD_PARTY_LICENSES.md`.

Only ordinary Win32 window messages are translated to `egui`. Device API hooks
belong to the game adapter: `mhf-launcher` filters DirectInput's immediate mouse
and keyboard samples using the shared capture handle. A host using Raw Input or
other polling APIs needs its own adapter for those paths.
