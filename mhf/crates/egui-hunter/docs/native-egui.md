# egui 原生能力核查

基于 workspace 锁定的 egui / eframe / epaint **0.36.1** 源码，核对 0.34–0.36 changelog。
版本依据为固定 tag 的官方
[egui changelog](https://github.com/emilk/egui/blob/0.36.1/CHANGELOG.md)、
[eframe changelog](https://github.com/emilk/egui/blob/0.36.1/crates/eframe/CHANGELOG.md) 和
[epaint changelog](https://github.com/emilk/egui/blob/0.36.1/crates/epaint/CHANGELOG.md)。

## 已交给原生

| 能力 | 当前实现 |
| --- | --- |
| 0.34 根 Ui、`Context::run_ui` | `Overlay::ui` 直接接收宿主创建的 `&mut Ui`。原生 Grid、Panel、布局作用域可直接组合。 |
| 原生容器背景命中 | VirtualList 和普通滚动视口用 `UiBuilder::sense` 注册原生焦点；Panel 只负责标题、背景和布局。选择列表的行使用 `Sense::CLICK`，列表提供一个键盘焦点；Tab 按原生顺序遍历实际控件，鼠标直接命中控件。 |
| 原生滚动布局与输出 | 滚动配置和输出直接使用 `ScrollArea` / `ScrollAreaOutput`。ScrollPanel 只组合主题 Panel、原生 ScrollArea 和列表导航策略。 |
| 原生容器配置 | Window 和 Popup 公开 `native`；位置、尺寸、ID、移动、缩放、关闭策略由原生 builder 配置。PopupInteraction 保留调用者的 `close_behavior`。 |
| 0.34 TextEdit Atom 前缀 | 图标作为原生 prefix，编辑器负责测量、排版与裁剪。 |
| 0.34 ScrollArea 边缘淡出、UiStack 背景 | Panel 在 UiStack 登记真实表面底色，原生淡出能识别皮革和羊皮纸。实际切角仍由 Shape 绘制，没有额外矩形底板。 |
| 原生 Style / Visuals | Theme 只安装原生 Style 预设。组件直接读取 Ui 的字体、间距、颜色、交互态及局部覆盖。Tokens 仅保留原生无法表达的专用值；独立 Area 显式传递局部样式。 |

## 已有能力与保留边界

| 核查项 | 结论 |
| --- | --- |
| eframe `logic` / `ui` | 启动器已经在 `logic` 接收异步结果，在 `ui` 渲染和派发界面动作。当前 `ui` 参数是 `&mut Ui, &mut Frame`。D3D9 Overlay 由游戏 Present 驱动，不引入第二套 eframe/winit 生命周期。 |
| 0.34 原生 Panel、0.35/0.36 面板拖动开关 | 原生 `egui::Panel` 用于屏幕边缘占位、缩放和折叠；本库 Panel 是带标题与切角的内容表面。布局直接使用原生 API，内容表面保留为设计系统组件。 |
| 多 Viewport | 额外 OS 窗口需要 backend 实现创建、输入和渲染。当前游戏宿主使用 egui 的嵌入回退；游戏内 Window 不等于 OS Viewport。 |
| 0.35 Window 标题 Atoms、0.36 `title_frame` | 原生标题可扩展，但 Frame 仍不能表达当前切角表面与主题关闭按钮。保留薄的 Window 视觉组合，位置、缩放和生命周期由原生 Window 执行。 |
| Popup / Modal / Area / Memory | 已用原生开关、定位、模态阻挡和焦点。保留初始焦点、关闭返回锚点、通知 FIFO/过期等产品交互；原生容器没有这些等价策略。 |
| 0.36 Popup 重开测量、非交互 Tooltip 修复 | 直接使用原生行为；不额外缓存 popup 尺寸或重建 tooltip 命中。 |
| 0.35 IME、0.36 修饰键 Event、TexturesDelta | 编辑器使用原生 TextEdit；平台事件转译和纹理提交仍是 backend 职责，当前适配器已处理修饰键事件和纹理 delta。移动端 IME 改善不替代 Windows 输入适配。 |
| 0.36.1 `Sense::drag` 修复 | 锁定版本已包含，使用原生命中，不添加拖动补丁。 |
| 0.35 Classes | 当前提供类标记和查询，不是完整的 CSS 选择器/主题继承引擎；继续使用 Style、Visuals 和 Ui 作用域，不建立第二套样式系统。 |
| 自定义 Widget / epaint | 切角、图标、选中勾选和材质纹理由主题统一绘制。焦点使用控件自身的原生 active 样式；文本框背景和列表活动行明确适配，不追加全局覆盖层或布局占位。绘制使用原生 Sense、Response、Ui 作用域、Painter、Shape 和字体栅格化，不改变焦点或 Tab 顺序。 |
| 手柄 Focus Engagement | 页面可使用 `FocusEngagement::begin/show/navigate` 声明需要手柄进入／退出的区域。仅手柄方向选区、A 进入、B 返回；键盘 Tab/Enter/Esc 与鼠标保持原生交互，不禁用未进入区域或移除页面其他控件的焦点。 |
| ResponsiveColumns | 只保留按可用宽度换列与稳定内容 ID 的策略；实际布局由 `Ui::columns` 完成。普通横排、自动换行、网格直接使用 egui。 |
| VirtualList、FocusGroup、scroll_keyboard | 选择列表只注册一个原生焦点，只保存活动索引与滚动状态；EventFilter 声明自身方向键，AccessKit 暴露活动子项。键盘 Tab 一次离开列表，手柄进入／退出交给可选 Engagement。FocusGroup 保留独立控件的网格边界，scroll_keyboard 补充按住方向键滚动。 |

### 页面级模态边界

需要独占交互的页面在最外层使用原生 `Modal`，并列 Panel 只提供布局和主题外观。
原生 Modal 负责背景交互隔离、顶层模态优先级和关闭判断；`UiBuilder::closable()` /
`ui.close()` 负责向最近的可关闭容器发出请求，`EventFilter` 让当前控件保留特定按键。

页面持有显示状态并处理业务结果，既有 `DialogInteraction` 执行模态关闭和关闭后的
入口焦点恢复。键盘可直接聚焦列表或按钮；Esc 先由编辑器、子 Modal 和 Popup 处理，
再交给父页面。普通浮窗继续使用原生 Window。

可选的 `FocusEngagement` 只负责手柄。它参考 [Xbox/UWP Focus Engagement](https://learn.microsoft.com/en-us/windows/uwp/ui-input/gamepad-and-remote-interactions#focus-engagement)
的 A 进入、B 退出模式；该指南明确不影响键盘及其他设备。普通 Panel 不自动接入，
页面自行选择哪些区域有必要提供此交互，布局嵌套不会建立通用 FocusScope。
手柄选区时，页面用原生 active 配色设置 `window_fill` / `window_stroke` 后绘制 Panel；
进入后突出实际控件，键鼠操作不经过区域框。

手柄宿主在首次 `NavigationInput::apply` 前安装 `EngagementPlugin`，在转换成 egui Key
前保留动作来源并路由。插件不会根据 Tab、Enter 或 Esc 键值把物理键盘误判为手柄。
`FocusEngagement::begin/show/navigate` 登记当前区域及控件，`Theme::apply` 只安装样式，
不会全局启用手柄规则；没有登记区域的页面保留原生导航。

Focus Engagement 的适用范围限定为 `ViewportId::ROOT`。同一根 viewport 内的原生
Window、Modal 和 Popup 正常支持；额外的原生操作系统 viewport 完全回退为原生导航，
不读取或修改根 viewport 的 Engagement 状态。egui 0.36.1 的插件 input/output hooks
位于 viewport 栈切换前后，且没有公开的按 viewport 访问 Memory 的接口，因此当前实现
明确保持这个边界；`NavigationInput` 的普通手柄按键适配不受此限制。

Modal 的输入隔离只作用于 egui。MHF 验证页显示期间由 Overlay 持续阻断鼠标和键盘，
隐藏时全部穿透；不会根据控件短暂失焦或鼠标离开面板就把输入交回游戏。

### 为什么没有采用 0.35 的输入区域快照

`Context::interactive_rects_last_pass()` 可提供裁剪、变换后的交互矩形，适合部分穿透宿主。
但在当前 0.36.1，实测 `Area::interactable(false)` 内的按钮仍出现在结果里，原生指针命中
却跳过该 Area；返回值只有 Rect，没有所属 LayerId，无法可靠区分上下层重叠的矩形。

因此保留现有 Area 输入快照和「按下到释放保持同一接收方」的宿主策略，避免通知使游戏
输入被错误拦截。`Auto` 仍用于浮动 Window/Area，直接根 Ui 的交互页面应由调用者选
`InputCapture::Block`。强制禁用子控件会改变 Area 的展示和交互契约，矩形包含关系也不能
可靠识别所属层。相关穿透契约由 overlay 的 capture 测试验证。

## 原生配置

```rust
let mut window = Window::new("装备").open(&mut open);
window.native = window.native
    .id(egui::Id::new("equipment"))
    .default_size([640.0, 480.0])
    .resizable(true);
window.show(ctx, |ui| {
    let mut engagement = FocusEngagement::new(egui::Id::new("equipment-engagement"));
    engagement.begin(ui, None); // 登记可选的手柄区域，键鼠不受影响。
    let region = egui::Id::new("records-region");
    engagement.show(ui, region, |ui, controls| {
        if ui.memory(|memory| memory.has_focus(region)) {
            let active = ui.visuals().widgets.active;
            ui.visuals_mut().window_fill = active.bg_fill;
            ui.visuals_mut().window_stroke = active.bg_stroke;
        }
        let mut panel = ScrollPanel::new(egui::Id::new("records"), "记录");
        panel.scroll = panel.scroll.max_height(260.0).scroll_bar_visibility(
            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
        );
        let response = panel.show_list(ui, 36.0, 10_000, |_| true, |ui, row| {
            ui.add_sized([ui.available_width(), 36.0], Button::new(&row.to_string()).sense(egui::Sense::CLICK))
        });
        // response.response：展示面板；response.inner.response：列表控件。
        // response.inner.scroll：原生 ScrollAreaOutput；response.inner.activated：本帧确认的条目。
        controls.push(response.inner.response);
    });
    engagement.navigate(ui);
});

let anchor = ui.add(Button::new("菜单"));
let mut popup = Popup::new(&anchor).title("行动");
popup.native = popup.native
    .id(egui::Id::new("actions"))
    .width(280.0)
    .close_behavior(egui::PopupCloseBehavior::CloseOnClick);
popup.show(|ui| { ui.label("内容"); });
```

Popup 的主题交互仍使用原生 Memory 开关；不要把 `native` 改为 `open_bool` / 始终开启，
否则不满足关闭后返回锚点的状态约定。需要不同开关策略时直接组合原生 Popup。
