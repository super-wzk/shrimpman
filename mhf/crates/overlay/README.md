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
    fn ui(&mut self, context: &egui::Context) {
        egui::Window::new("Packet debugger").show(context, |ui| {
            ui.label("Ready");
        });
    }
}

fn install() -> Result<D3d9Hook, mhf_overlay::Error> {
    unsafe { D3d9Hook::install(DebugUi) }
}
```

## Upstream references

The hook discovery and lifecycle follow the design of
[`hudhook`](https://github.com/veeenu/hudhook). The renderer and input boundary
were written against the current workspace `egui` version, using
[`egui-d3d9`](https://github.com/unknowntrojan/egui-d3d9) as a behavioral
reference. Both upstream projects are MIT licensed; their license notices are
recorded in `THIRD_PARTY_LICENSES.md`.

Only ordinary Win32 window messages are translated to `egui`. A game using
Raw Input or DirectInput may still receive input even while `egui` wants it;
intercepting those APIs is intentionally outside this rendering crate.
