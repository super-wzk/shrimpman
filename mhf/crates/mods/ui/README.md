# mhf-ui

默认导出 [`api`](src/api/mod.rs) 和同名根入口：`UiHost` 绑定 `mhf.ui.v1` 并注册 `Panel`，
回调借用 `Ui` 调用 label、button、checkbox。API 使用同一份 Rust 类型生成 C 布局，
不依赖渲染器或提供方实现；`headers` feature 开启 `api::define_header`，由启动应用的构建脚本聚合。
接口由 `mhf.base` 发布；消费者声明对 Base 的依赖。

`provider` feature 提供 `OverlayRegistry`、`Overlay` 和输入捕获等协作类型，
在 Windows 上提供 `UiMod`，内部包含 D3D9、窗口／IME 与 DirectInput 后端。
内部 `UiService` 持有稳定的公开表；`UiMod` 由 [`mhf-base`](../base/README.md) 组合运行，实现内部 `Module` 生命周期，不作为独立运行时 Mod。

```rust,ignore
let ui_host = mhf_ui::UiHost::bind(host.dependencies())?;
let mut panel = ui_host.panel("状态", |ui| {
    let _ = ui.label("已连接");
})?;
// Mod::stop 传播失败，保留仍可能被调用的回调和 DLL。
panel.close()?;
```

`UiMod::new(registry, ime_adapter, capture)` 接受共享注册表、
`Rc<RefCell<Option<Arc<dyn HostIme>>>>` 和 `Rc<RefCell<Option<InputCaptureState>>>`。
这些协作对象由 Base 创建，内置调试面板通过 Base 的 `registry()` 访问同一注册表。
当前 Base 不注册游戏原生编辑器；UI 在 attach 阶段安装渲染和 DirectInput 后端，
成功后发布输入捕获状态。
UI 的安装失败会保留未能回滚的 Hook；stop 失败可重试，成功后清除共享捕获句柄。
`HostIme` 保留为可选协作接口，当前应用不安装 Unicode 游戏 IME 适配器。

## Rendering backend

`D3d9Hook` installs process-local Direct3D 9 `Present` and `Reset` hooks and renders
an `egui` interface into the hooked device. It is linked into the existing game
process; DLL injection is outside the provider.

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
use mhf_ui::{Overlay, dx9::D3d9Hook, egui};

struct DebugUi;

impl Overlay for DebugUi {
    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Window::new("Packet debugger").show(ui.ctx(), |ui| {
            ui.label("Ready");
        });
    }
}

fn install() -> Result<D3d9Hook, mhf_ui::Error> {
    unsafe { D3d9Hook::install(DebugUi) }
}
```

## Caller-controlled input capture

Override `Overlay::input_policy` to choose mouse and keyboard behavior independently:

```rust
use mhf_ui::{InputCapture, InputPolicy, Overlay, egui};

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
settings govern forwarding to the game's original window procedure. IME sessions
are exclusive: `PassThrough` leaves composition with the host instead of sending
the same composition to two editors.

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

## Input methods

The backend uses a private IMM32 context while the overlay captures keyboard input.
Without a host adapter, releasing keyboard capture restores the original native
context and forwards IME messages to the game, including before the first overlay
editor opens. A blocking modal with no text focus temporarily suspends IME.

With a native host adapter, egui and the native editor share one private context
until the window binding is removed. With no focused editor, the window is associated
with a null context so IME does not consume gameplay keys; the private context keeps
its input mode for the next editor. Returning input ownership or removing the binding
restores the exact original association, including an originally null context.

Use `D3d9Hook::install_with_ime(overlay, Arc<dyn HostIme>)` to connect native text
controls. `HostIme::target` supplies the current editor identity and a cursor
rectangle in client pixels. `HostIme::event` receives preedit/commit events on the
window thread while the shared context is still associated. A legacy adapter can
read the original ANSI composition from that context and retain its existing
encoding, buffers and insertion routine. Game-specific addresses stay in the
adapter. Ordinary `WM_CHAR` messages keep their existing host path.

Keyboard capture gives the overlay priority. A focused egui text field receives
UTF-16 composition/result strings as `ImeEvent::Preedit` and `ImeEvent::Commit`,
including the active clause or cursor range. A blocking modal without a text field
also excludes the native editor. With `PassThrough`, the native editor owns IME
even when egui has a focused field. Each editor draws its own preedit; the system
input method draws candidates at its cursor. Handled composition messages bypass
the original game procedure and cannot generate duplicate character events.

The backend also synchronizes a hidden Win32 caret with the active text cursor.
WineCX's macOS driver reads that caret through `GetGUIThreadInfo` to position
native candidates; updating the IMM candidate/composition forms alone is not
sufficient. Caret coordinates follow UI scaling in client pixels and are mapped
to screen coordinates by Wine. An existing host caret keeps its shape/visibility
and regains its previous position when input ownership ends.

Changing owners cancels composition and delivers cancellation to the previous
editor before switching. Composition navigation/edit keys are withheld from the
host window procedure while device polling retains the caller's capture policy.
Frame callbacks post updates; all IMM operations and host adapter callbacks run
on the window thread without holding overlay locks. Uninstall on that thread, or
keep it pumping messages until synchronous uninstall returns.

Native tests cover handle reuse across host/overlay/idle transitions, cancellation
recipients, modal/pass-through priority, context restoration, Unicode input,
composition key routing and candidate coordinates. Actual candidate selection and
text entry still need verification with the selected Windows or macOS/Wine input
method.

## Upstream references

The hook discovery and lifecycle follow the design of
[`hudhook`](https://github.com/veeenu/hudhook). The renderer and input boundary
were written against the current workspace `egui` version, using
[`egui-d3d9`](https://github.com/unknowntrojan/egui-d3d9) as a behavioral
reference. Both upstream projects are MIT licensed; their license notices are
recorded in `THIRD_PARTY_LICENSES.md`.

Win32 window messages feed `egui`; IMM32 composition goes exclusively to the active
overlay or host editor. The UI provider's native adapter filters DirectInput's
immediate mouse and keyboard samples using the shared capture handle. A host
using Raw Input or other polling APIs needs its own adapter for those paths.

## 游戏内界面与输入法

`mhf.base` 的 UI 组件通过 Overlay 后端协调游戏输入与原生输入法；未注册额外面板时，鼠标和键盘穿透给游戏。Debug Mod 通过同一宿主显示 F7 调试窗口，按 egui 的交互状态
自动决定输入捕获。输入策略改变时，已按下的按钮和按键保持原接收方直到松开。

MHF 输入适配层拦截 [`GetDeviceState`](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee417897(v=vs.85))
返回的鼠标和键盘状态，使用与窗口消息相同的策略和原始按键归属。鼠标支持 16/20 字节
标准状态，捕获时过滤按钮、相对移动和滚轮；键盘使用 256 字节扫描码状态。设备类型和
数据长度必须匹配，手柄、未知格式和失败调用保留原样。Hook 地址从运行时设备虚表
获取，不依赖游戏 DLL 的固定地址，并在 `mhDLL_Main` 返回后先于 D3D9 Overlay 卸载。

D3D9 渲染、窗口输入和游戏本身的输入接收需要在实际游戏环境中检查。
当前 Base 未注册游戏原生编辑器适配器，游戏输入框保留原有处理；Overlay 使用 UI 自己的输入法管理。
`HostIme` API 仍可供其他提供方注册编辑器。真实选词和上屏需要在 Windows/macOS Wine 中验证。
手柄以及 RawInput 的输入仲裁仍由宿主负责。
