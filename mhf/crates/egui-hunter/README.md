# egui-hunter

基于 egui 0.36.1 的猎人工坊风格组件库。统一使用炭黑皮革、骨白文字、旧黄铜、苔绿和少量羊皮纸；面板与控件共用切角、细边框和角部纹样。

核心仅依赖 `egui`，可用于原生窗口、游戏内 overlay 或其他 egui 宿主。窗口、渲染后端、中文字体、纹理资源和业务状态由宿主提供。`eframe` 和 PNG 编码仅用于开发示例。

## 使用

```rust
use egui_hunter::{ButtonKind, Icon, Theme};

let theme = Theme::default();
// 初始化时调用一次，保留宿主已安装的字体。
theme.apply(&context);

// 每帧：
theme.panel("任务列表").show(ui, |ui| {
    if ui.add(theme.button("接受任务")
        .kind(ButtonKind::Primary)
        .icon(Icon::Quest)).clicked()
    {
        // 由宿主处理任务状态。
    }
    ui.add(theme.checkbox(&mut show_tips, "显示提示"));
    ui.add(theme.toggle(&mut auto_sort, "自动整理"));
    ui.add(theme.slider(&mut volume, 0.0..=100.0).text("音量"));
});
```

`Theme` 是可复制的普通值，`palette` 和 `metrics` 可定制。修改主题后再次调用 `apply`，使 egui 标准控件和自绘组件保持一致。

## 组件

| 组件 | API | 行为 |
| --- | --- | --- |
| 面板 / 羊皮纸 | `theme.panel(title).surface(Surface::Parchment).show(...)` | 内容自适应高度、标题、纹理与装饰边框 |
| 按钮 | `theme.button(label).kind(ButtonKind::Primary)` | 普通、主要、危险，悬停、聚焦、按下、禁用 |
| 选择行 | `theme.button(label).selected(value).full_width()` | 苔绿填充与菱形标记；焦点独立显示 |
| 复选框 / 开关 | `theme.checkbox(...)` / `theme.toggle(...)` | 点击或 Space/Enter 切换，返回 `changed()` |
| 搜索框 | `theme.text_edit(ui, id, &mut text, hint)` | 带搜索图标的输入字段，保留原生选择、剪贴板、IME |
| 输入字段 | `theme.text_field(id, &mut text).label(...).validation(...)` | 标签、占位、帮助、成功/警告/错误、密码、可复制的只读、禁用 |
| 富提示框 | `theme.tooltip(&response, title).show(...)` | 延迟悬停或键盘聚焦显示，可组合说明、图标、属性；适配屏幕边缘 |
| 属性列表 | `theme.properties(ui, &[Property::new(label, value)])` | 标签/数值对齐、长文本换行、窄宽度上下排列，支持数值强调色 |
| 滑块 | `theme.slider(&mut value, range)` | 窄矩形滑块，保留原生拖拽与数值编辑 |
| 物品格 | `theme.item_slot(label).icon(...).quantity(...).selected(...)` | 选中、数量、提示；`.image(TextureId)` 接入宿主图标 |
| 状态条 | `theme.meter(fraction).label("体力").color(...)` | 归一化进度，非有限值显示为空 |
| 输入提示 | `theme.key_hint(ui, "A", "确认")` | 统一键帽外观，宿主决定设备与绑定 |
| 通知 | `theme.notice(ui, NoticeKind::Success, text)` | 成功、警告、危险；颜色配合语义图标 |
| 图标 | `Icon::paint(painter, rect, color)` | 委托、武器、回复药、药草、骨、矿石、陷阱等矢量图标 |

## 容器、布局与界面状态

| 能力 | API | 行为 |
| --- | --- | --- |
| 浮动窗口 | `theme.window(title).open(&mut open)` | 统一标题/关闭按钮；原生拖动、边缘缩放、位置与层级 |
| 滚动面板 | `theme.scroll_panel(id, title)` | 标题固定、内容裁剪；`show_rows` 虚拟化等高长列表；聚焦后按住方向键连续滚动 |
| 可交互虚拟列表 | `theme.scroll_panel(id, title).show_list(...)` | 按完整列表索引移动焦点，自动滚动到屏外条目，跳过禁用项；支持上下、翻页、首尾导航 |
| 对话框 | `theme.dialog(id, title)` + `OverlayState` | 模态遮罩、初始焦点、确认/取消、Esc、可配置点击背景关闭 |
| 弹出菜单 | `theme.popup(&anchor)` + `OverlayState` | 锚点点击切换、自动调整位置、点击外部关闭；内部通过 `ui.close()` 收起 |
| 响应式分栏 | `theme.columns(id).min_column_width(400.0)` | 等宽分栏、窄屏堆叠；默认最多两栏，重排时保留子控件 ID |
| 页签容器 | `theme.tabs(id)` + `TabsState` + `Tab` | 选中内容作用域、禁用页签、左右/Home/End 导航；移除当前页后自动回退 |
| 菜单栈 | `MenuStack<Page>` | 压入页面、逐级返回、根页保护、返回后恢复入口焦点 |
| 通知队列 | `Notifications` | 有界 FIFO、逐条展示、到期/手动关闭、清空；使用 egui 时间和重绘调度 |
| 方向导航 | `FocusGroup::vertical()` / `FocusGroup::grid(columns)` | 按行列移动、跳过禁用项、边界停留或循环、滚动至目标；Tab 保留原生顺序 |
| 手柄动作适配 | `NavigationInput` + `GamepadState` | 四方向连发、确认/返回单次触发、前后焦点、最近输入设备、重绘调度 |

布局计算、尺寸协商、裁剪和层级仍交给 egui；库内统一外观与交互约定。行列、网格、分隔线、文字、图片、单选框和下拉框可以继续组合 egui 标准 API，继承主题。状态对象由宿主持有，核心不包含游戏业务、后台线程或系统输入采集。

```rust
// 每帧根据可用宽度排列三个面板；id 在整个 egui Context 内唯一。
theme.columns(egui::Id::new("overview"))
    .min_column_width(400.0)
    .max_columns(3)
    .show(ui, 3, |ui, index| {
        theme.panel(["装备", "道具", "委托"][index]).show(ui, |ui| {
            ui.label("面板内容");
        });
    });

theme.scroll_panel(egui::Id::new("archive"), "档案")
    .max_height(260.0)
    .show_rows(ui, 36.0, 10_000, |ui, visible_rows| {
        for row in visible_rows {
            ui.add_sized([ui.available_width(), 36.0],
                egui::Label::new(format!("档案 {row}")));
        }
    });
```

`show_rows` 的行高不包含行间距，调用方应保持每行等高。分栏中的索引代表稳定位置；如果数据会排序或增删，内容控件另用稳定业务 ID。`ScrollPanel` 的滚动 ID 相对于父 UI；分栏、菜单页和页签内容提供稳定父作用域。

滚动面板可通过 Tab 或点击内容空白处聚焦，黄铜角标标记当前滚动区域。聚焦区域本身时，按住 Up/Down 连续滚动，PageUp/PageDown 翻页；焦点在子文本框或按钮上时保留子控件行为。已有的原生 `egui::ScrollArea` 可在内容闭包末尾调用 `theme.scroll_focus(ui, id)` 接入相同行为，返回的 `Response` 也可用来显式请求焦点。内层滚动区独立消费自己的按键，不同时滚动外层。

如果列表中的每一行是可选择或可点击的条目，使用 `show_list`。它按整个列表的索引处理 Up/Down、PageUp/PageDown、Home/End；到达可见区域末尾时先滚动到目标行，再将焦点交给该行。到达真正的列表首尾时保留焦点，Tab 继续沿原生顺序移动。

```rust
theme.scroll_panel(egui::Id::new("quest-list"), "委托档案")
    .max_height(258.0)
    .show_list(ui, 36.0, 10_000, |_| true, |ui, row| {
        let response = ui.add_sized([ui.available_width(), 36.0],
            theme.button(&format!("第 {:05} 号委托", row + 1)));
        if response.clicked() {
            // 查阅委托。
        }
        response
    });
```

第一个闭包查询某行是否启用，示例中的 `|_| true` 表示全部可用；第二个闭包只渲染可见行，返回该行的选择控件 `Response`。禁用状态由容器统一应用，查询屏外条目的状态不会创建其控件。普通内容、包含文本编辑器等多个控件的行继续使用 `show_rows`。

```rust
use egui_hunter::OverlayState;

// 放在宿主状态里，每个浮层一个实例，不要每帧重新创建。
let mut dialog = OverlayState::default();

// 每帧：先画入口，再画对话框。
let opener = ui.add(theme.button("打开确认"));
if opener.clicked() {
    dialog.open_from(&opener);
}
let confirm = egui::Id::new("confirm-action");
theme.dialog(egui::Id::new("confirmation"), "确认操作")
    .initial_focus(confirm)
    .show(ui.ctx(), &mut dialog, |ui| {
        if ui.add(theme.button("确认").id(confirm)).clicked() {
            ui.close();
        }
    });
```

即使浮层已关闭，也继续调用 `show`，使入口重新渲染后能恢复焦点；宿主也可调用 `OverlayState::open/close`。关闭弹出菜单的外部点击保留新点击位置的焦点。弹出菜单使用 egui 的原生单菜单状态：打开另一个菜单会关闭旧菜单。

菜单页键需实现 `Debug + Eq + Hash`。`MenuStack::show` 渲染当前页，入口响应传给 `push_from(&response, next_page)`；返回按钮或手柄 B 动作调用 `back(ctx)`。在 `show` 之后提交的导航下一帧生效。每帧按主界面、外层对话框、内层对话框顺序渲染；Esc 优先关闭当前弹出菜单/顶层对话框，然后才返回菜单页。多个菜单同时可见时，宿主应只对当前活动菜单启用输入。

`Notifications::new(id)` 默认最多 32 条，一次显示一条；`push(ctx, kind, text)` 默认显示 3 秒，`push_for` 自定义时长。**排队中的消息从首次展示开始计时**。达到容量时移除最早等待的消息，保留当前可见消息；容量为 1 时替换当前消息。通过返回 ID 调用 `dismiss`，或调用 `clear`。主界面绘制后调用 `notices.show(ctx, &theme)`。

## 信息组件与输入状态

```rust
use egui_hunter::{Property, Validation};

let validation = if name.trim().is_empty() {
    Validation::Error("请填写猎人姓名")
} else {
    Validation::Success("姓名可以使用")
};
let response = ui.add(theme.text_field(egui::Id::new("name"), &mut name)
    .label("猎人姓名").hint("输入姓名")
    .help("可使用中文").validation(validation));

let item = ui.add(theme.item_slot("铁刀").icon(Icon::Sword).hover_text(false));
theme.tooltip(&item, "铁刀 · 锻造资料").show(|ui| {
    ui.label("工坊以精炼矿石打造的武器。");
    theme.properties(ui, &[
        Property::new("攻击力", "528 (+48)").color(theme.palette.moss),
        Property::new("锻造费用", "2400 z"),
    ]);
});
```

字段返回原生编辑器的 `Response`，标签和帮助文本不会改变显式 ID。校验由调用方决定，校验消息优先于帮助文本，并同时显示文字、颜色和图标。`.read_only(true)` 保留文本选择与复制；`.password(true)` 继承 egui 的遮掩和禁止复制行为；`ui.add_enabled(false, field)` 禁止交互，也会移除原有编辑焦点。现有 `text_edit` 搜索接口复用同一套绘制和编辑实现。

富提示框沿用 egui 的悬停延迟与定位，不开启菜单状态，也不主动改变焦点。禁用控件也可附上原因说明；`.on_focus(false)` 可关闭聚焦展示。给物品格附加富提示框时用 `.hover_text(false)` 关闭默认的纯文字提示。属性列表继承父面板的文字颜色，羊皮纸中也可使用；单位、差值和本地化文案由宿主格式化。极长内容应由调用方在提示框内组合滚动区域。

## 方向与手柄导航

```rust
use egui_hunter::FocusGroup;

let controls = [
    ui.add(theme.button("森林探索")),
    ui.add_enabled(false, theme.button("未解锁")),
    ui.add(theme.button("采集与调合")),
];
FocusGroup::vertical().wrap(true).navigate(ui, &controls);
```

每帧先渲染再传入同组的 `Response`，网格按行优先排列，禁用项也要保留原位置。焦点和选中值独立：方向键只移动焦点，Space/Enter 确认才修改宿主状态。默认边界停留，`.wrap(true)` 在同一行/列循环；不完整末行不会跳到其他列。只注册需要方向选择的控件，文本编辑器和滑块继续处理自己的方向键。组外控件仍使用 egui 原生几何导航；Tab/Shift-Tab 可以离开组。可交互虚拟列表使用 `ScrollPanel::show_list`，它会处理尚未渲染行的导航。

鼠标导航按钮可以调用 `FocusGroup::move_focus(ui, &controls, from_id, direction)`，无需伪造手柄输入。宿主应独立保存导航位置和已选值，避免鼠标释放清除焦点后跳回已选项；鼠标点击也不应改变手柄设备提示。

```rust
use egui_hunter::{Direction, GamepadState, NavigationInput};

// 宿主持有一个 NavigationInput，每个 viewport 一个实例。
let mut navigation = NavigationInput::default();
// 每帧在 Context::run 前调用；eframe 使用 App::raw_input_hook。
navigation.apply(&context, &mut raw_input, GamepadState {
    direction: Some(Direction::Down), // 按住的状态，不是每帧重新触发的事件
    confirm: false,
    cancel: false,
    next_focus: false,     // 可映射右肩键，相当于 Tab
    previous_focus: false, // 可映射左肩键，相当于 Shift-Tab
});
```

方向首次按下立即触发，默认延迟 350 ms 后每 90 ms 连发，`repeat_timing` 可调整。其余动作仅在按下边沿触发；松开或断开设备时传默认状态。没有焦点时，方向或确认会先进入原生焦点顺序。窗口失焦期间忽略动作，恢复窗口不会重新触发仍按住的确认键；按键脉冲不会释放真实键盘上仍按住的同名键。`device()` 返回最近输入设备，供宿主选择键帽文案。适配器只处理宿主交给 UI 的动作，系统手柄采集、摇杆死区和游戏/UI 输入分配仍由宿主完成。

## 交互约定

- **选中**：苔绿填充 + 菱形标记，表示当前值。
- **聚焦**：黄铜角标，表示接收键盘操作的位置；可以和选中同时存在。
- **危险**：朱红边框 + 警告图标；禁用统一使用 `ui.add_enabled(false, widget)`。
- 自绘交互组件注册 egui 的可访问性元信息，沿用 egui 的点击、Tab、Space/Enter 和禁用行为。
- 动态列表使用 `ui.push_id(stable_item_id, ...)`，使筛选和排序后的控件身份保持稳定。
- 坐标均为 egui 逻辑点，宿主负责 DPI/缩放；默认采用 8 点间距、36 点操作高度。
- 中文字体由宿主用 `Context::set_fonts` 安装。组件库不读文件、不注册系统字体。
- 手柄键帽按宿主绑定显示；`NavigationInput` 可将标准导航动作映射为 egui 输入，实际设备采集与游戏/UI 输入仲裁由宿主完成。

## 可交互展厅

在 `mhf` 目录运行。工作区默认编译 Windows i686，运行本机示例时需要显式指定宿主 target。

```sh
# 此工作区的 macOS Apple Silicon 宿主
cargo run -p egui-hunter --example gallery --target aarch64-apple-darwin

# 指定中文字体；Windows / Linux 使用相应的宿主 target
cargo run -p egui-hunter --example gallery --target aarch64-apple-darwin -- \
  --font /path/to/chinese-font.ttf

# 实际 egui 渲染截图，保存后自动退出
cargo run -p egui-hunter --example gallery --target aarch64-apple-darwin -- \
  --screenshot /tmp/hunter-ui.png
```

示例优先使用 `--font`，否则尝试常见的系统中文字体；字体不会打包进库。`--compact` 切换窄窗口，`--dialog` 打开任务确认弹窗。`--containers` 直接进入容器与导航页，`--popup` / `--window` 同时打开该页的弹出菜单 / 浮动窗口，`--notices` 播放消息队列。`--details` 进入信息与交互页，`--tooltip` 同时聚焦装备图标以展示富提示框，便于截图检查。

组件页包含任务确认、背包分类/搜索/整理、道具消耗与体力更新、禁用状态、键鼠/手柄提示和 HUD。容器页包含三级营地菜单、一万条委托档案、浮动手记、行动菜单、确认对话框和消息队列。信息页包含登记表、装备属性与富提示框、装备选择和只读/禁用/校验状态；装备选择下方的向上/向下按钮直接移动焦点并支持按住连发，确认后切换装备。示例未采集真实手柄。窄窗口自动堆叠面板并允许纵向滚动。

## 验证

```sh
cargo test -p egui-hunter --all-targets --target aarch64-apple-darwin
cargo test -p egui-hunter --doc --target aarch64-apple-darwin
cargo clippy -p egui-hunter --all-targets --target aarch64-apple-darwin -- -D warnings
cargo check -p egui-hunter --lib --target i686-pc-windows-msvc
```

测试通过合成 egui 输入检查点击、键盘、禁用、中文编辑、只读复制与密码限制、方向边界/连发/焦点切换、提示框延迟/定位、窗口移动/缩放/关闭、浮层关闭顺序与焦点恢复、长列表虚拟化和滚动位置、响应式 ID、页签导航及通知生命周期。截图来自真实 eframe/egui 渲染；系统 IME、真实手柄和实际游戏内输入仍需在对应宿主验证。
