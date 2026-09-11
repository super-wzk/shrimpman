# egui-hunter

基于 egui 0.36.1 的现代狩猎界面组件库。统一石墨灰表面、浅色文字和克制的金色强调；控件圆角 8px、面板圆角 12px。选中填充与勾选保留当前值，原边框的焦点强调显示键盘或手柄目标。

核心使用 `egui` 和既有 `image` PNG 解码能力，可用于原生窗口、游戏内 overlay 或其他 egui 宿主。窗口、渲染后端、中文字体和业务状态由宿主提供。14 类武器图标保留客户端原始颜色与像素，`eframe` 仅用于开发示例。

## 分层与目录

设计系统同时包含交互规范和视觉规范。通用机制按职责组织，组件内部按需要分离交互和绘制：

```text
src/
├── input.rs                      # 手柄动作适配、按键消费
├── primitives/
│   ├── focus.rs                  # 原生焦点分组、恢复及公共交互约定
│   ├── focus/engagement.rs       # 按页面选择启用的手柄 Focus Engagement
│   ├── navigation/
│   │   ├── mod.rs                # NavigationState：内容选择与稳定作用域
│   │   └── stack.rs              # NavigationStack：进入、返回、根页保护
│   └── layout/
│       ├── columns.rs            # ResponsiveColumns
│       ├── scroll.rs             # 原生 ScrollArea 的键盘滚动策略
│       └── list.rs               # VirtualList
├── theme/                        # Theme、tokens、icons、paint
└── components/
    ├── controls.rs / fields.rs / select.rs / information.rs
    ├── form/                     # Field 字段说明、FormLayout 行列对齐
    ├── panel.rs / item_slot.rs / window.rs / scroll_panel.rs
    ├── tabs/ / dialog/ / popup/ / tooltip/
    │   ├── interaction.rs        # 组件专属状态、交互策略，无 Theme 依赖
    │   └── view.rs               # 风格化绘制与组合
    └── notifications/
        ├── queue.rs              # 消息状态、容量和计时
        └── view.rs               # 浮动通知的展示
```

`primitives` 不引用主题或具体组件。`ScrollPanel::show_list` 组合主题 `Panel`、原生 `egui::ScrollArea` 和 `VirtualList`。列表是一个原生控件，只保存活动行和滚动状态。`interaction` 中的组件策略也可以搭配原生 Modal、Popup、Tooltip 或自定义页签头使用。

`Panel` 只负责标题、背景、边框和内容布局。键盘和鼠标保留原生 egui 交互：Tab 遍历实际控件，鼠标直接点击操作，普通面板不增加焦点入口。页面可用 `FocusEngagement` 显式登记适合手柄操作的区域；仅在手柄输入时，方向键先选区域，A 进入，B 返回。没有登记的页面保持原生行为，`Theme::apply` 不会自动启用这套手柄策略。列表仍只有一个原生控件焦点，活动索引与业务选中值分别保存。

```rust
use egui_hunter::primitives::layout::VirtualList;

let output = VirtualList::new(egui::Id::new("records")).show(
    ui,
    egui::ScrollArea::vertical().id_salt("records").max_height(260.0),
    36.0, 10_000, |_| true,
    |ui, row, current| {
        ui.push_id(row, |ui| {
            let mut button = egui::Button::new(format!("记录 {row}"))
                .sense(egui::Sense::CLICK);
            if current {
                button = button.fill(ui.visuals().widgets.active.weak_bg_fill);
            }
            ui.add_sized([ui.available_width(), 36.0], button)
        }).inner
    },
);
// output.response：容器的原生焦点；output.scroll：原生 ScrollAreaOutput。
// output.active：活动条目；output.activated：本帧确认或点击的条目。
```

`VirtualList` 通过原生 `UiBuilder::sense` 提供一个键盘焦点，聚焦后即可导航和确认，鼠标点击条目会聚焦列表并报告激活项。Tab 一次离开列表，不随活动条目增加停靠点；Esc 留给页面或弹层处理。页面将 `ListOutput.response` 登记到 `FocusEngagement::show` 后，手柄可先选整个区域、按 A 进入列表，再按 B 返回；列表本身不保存手柄进入状态。

布局、配置和输出直接使用 egui 类型。`ScrollPanel.scroll` 是原生 `ScrollArea`，`Window.native` / `Popup.native` 是原生容器配置；组件只补充主题和专属交互策略。0.34–0.36 的采用与保留依据见 [原生能力核查](docs/native-egui.md)。

浮动滚动条需要避让内容时，在使用处设置 `ScrollArea::content_margin` 的右侧边距；主题保留原生浮动策略。

`NavigationStack` 管理页面进入与返回，`NavigationState` 管理内容选择；两者均不解析路由。Dialog 的开关状态为 `DialogState`，Popup 开关与控件焦点使用原生 egui Memory。

## 使用

```rust
use egui_hunter::{Button, ButtonKind, Checkbox, Icon, Panel, Theme, Toggle};

let theme = Theme::default();
// 初始化时调用一次，保留宿主已安装的字体。
theme.apply(&context);

// 每帧：
Panel::new("任务列表").show(ui, |ui| {
    if ui.add(Button::new("接受任务")
        .kind(ButtonKind::Primary)
        .icon(Icon::Quest)).clicked()
    {
        // 由宿主处理任务状态。
    }
    ui.add(Checkbox::new(&mut show_tips, "显示提示"));
    ui.add(Toggle::new(&mut auto_sort, "自动整理"));
    ui.add(egui::Slider::new(&mut volume, 0.0..=100.0).text("音量"));
});
```

`Theme` 只负责安装预设，包含原生 `egui::Style` 和少量专用 `Tokens`。可在安装前修改 `theme.style`；安装后，原生控件和自绘控件都读取当前 `Ui`，不再持有主题引用。

### 界面密度

默认 `Density::Standard` 保持现有登录和游戏 UI 的尺寸。资源工具等需要更多可见内容的页面，可以只为自己的 UI 子树启用 `Density::Compact`：

```rust
use egui_hunter::{Button, Density, TextField};

Density::Compact.scope(ui, |ui| {
    ui.button("原生按钮");
    ui.add(Button::new("组件按钮"));
    ui.add(TextField::new(egui::Id::new("filter"), &mut filter));
});
// 后续兄弟控件仍使用父 Ui 的密度与样式。
```

| 默认尺寸（逻辑点） | 标准 | 紧凑 |
| --- | ---: | ---: |
| 普通控件最小高度 | 36 | 24 |
| 文本框 / 选择框最小高度 | 40 | 28 |
| 主要按钮最小高度 | 44 | 28 |
| 控件间距 X / Y | 8 / 6 | 6 / 4 |
| 按钮内边距 X / Y | 12 / 7 | 8 / 3 |
| 面板内边距 | 12 | 8 |
| 图标 / 图标内框 | 20 / 16 | 16 / 12 |

密度只调整布局尺寸；字体、字号、颜色、DPI 缩放与输入逻辑不变。文字或图标较大时控件仍会增高，显式 `.min_size(...)`、物品槽 `.size(...)` 和布局断点仍由调用方决定。`FormLayout` 的标签与控件共用字段高度；页签也考虑实际文字高度。`ResponsiveColumns` 默认间隙为当前横向 `item_spacing` 的两倍，显式 `.gap(...)` 优先。

独立宿主可以用 `Theme::default().density(Density::Compact).apply(&context)` 选择初始密度；同一 Context 中只有局部页面需要紧凑时，应使用 `Density::scope`，不要重复安装全局 Theme。`Density::get(ui)` 读取当前值。原生 spacing 和 hunter 特有的最小尺寸通过现有 Style / Tokens 继承：不要只修改 `Tokens::density` 而遗漏原生 spacing。

Window、Dialog、Popup 和 RichTooltip 创建独立 Area，跨边界仍需传递 `.style(ui.style().clone()).tokens(Tokens::get(ui))`，或在新 Area 的内容闭包内调用 `Density::scope`。`SelectField` 自己的菜单已传递局部密度。浮动通知可用 `notifications.show_in(ui)` / `show_at_in(ui, anchor, offset)` 继承调用处；原有 `show(ctx)` / `show_at(ctx, ...)` 继续使用宿主默认值。

```rust
ui.scope(|ui| {
    ui.visuals_mut().override_text_color = Some(egui::Color32::LIGHT_BLUE);
    ui.spacing_mut().button_padding = egui::vec2(20.0, 10.0);
    ui.add(Button::new("使用局部样式"));
    let _ = ui.button("原生按钮也使用同一份样式");
});
```

主要动作的前景/背景、成功的前景/背景、危险背景与焦点颜色没有完整的原生语义字段，保存在 `Tokens`。`Tokens::get(ui)` 读取最近的 Ui 标签，回退到 Context 中安装的默认值；`tokens.scope(ui, content)` 提供仅对后代生效的局部覆盖。通用颜色、字体、圆角和间距使用 `Style` / `Visuals`；仅没有原生 Style 字段的 hunter 控件最小高度由密度值补充。

控件保留默认、悬浮、按下和禁用状态。主要按钮保持金色底与深色文字，选择行保持选中底与金色勾选，聚焦不替换这些状态。按钮、物品槽和选择控件仅将原有内部边框加粗到 2px，不扩展矩形或叠加另一圈；复选框强调方框，开关强调轨道。金色主按钮和已勾选控件使用深色焦点边框，保留原有填充与文字。文本框沿用原生选择、游标和编辑行为，聚焦时仅加粗原有内部边框至 2px，并保留校验色；不追加外环。密码字段可用 `.password_visible(&mut visible)` 在框内显示可聚焦的可见性按钮，编辑文字由原生 suffix 布局避让。焦点强调不改变内容尺寸和位置。Tabs 和 ScrollPanel 保留组件自身的内部焦点框，分别标记页签与键盘滚动区域。

`VirtualList` 的行回调接收 `current`，表示容器拥有焦点时的活动条目，并非业务选中值。`ScrollPanel` 在绘制前通过原生 Ui 作用域传递该视觉状态和局部样式，行仍使用 `Sense::CLICK`；它不注册新的焦点，也不拦截 Tab。需要确认后回焦的下拉选择复用已有 `Popup` 关闭流程，与普通菜单共享策略。

Window、Dialog、Popup 和 RichTooltip 底层创建独立 Area，默认使用 Context 样式。需要继承调用处时，显式复制当前样式和专用值：

```rust
let anchor = ui.add(Button::new("菜单"));
Popup::new(&anchor)
    .style(ui.style().clone())
    .tokens(egui_hunter::Tokens::get(ui))
    .show(|ui| { ui.label("继承锚点所在作用域的样式"); });
```

`Surface::Panel` 使用 `window_fill`，`Surface::Raised` 使用 `faint_bg_color`。两者都是原生圆角 Frame，不切换明暗主题或改写子控件文字。Popup 和 RichTooltip 默认使用 Raised，Window 和 Dialog 使用 Panel。独立 Area 的注入遵循上述规则。

## 组件

| 组件 | API | 行为 |
| --- | --- | --- |
| 面板 | `Panel::new(title).surface(Surface::Raised).show(...)` | 内容自适应高度、标题、圆角与装饰边框 |
| 按钮 | `Button::new(label).kind(ButtonKind::Primary)` | 普通、主要、危险、Quiet、DangerQuiet；普通高 36px，主要高 44px |
| 页签 | `Tabs::new(id).show(...)` | 文字导航与金色底线；保留左右键、Home/End 和点击切换 |
| 选择行 | `Button::new(label).selected(value).full_width()` | 深金色填充与金色勾选标记；焦点独立显示 |
| 复选框 / 开关 | `Checkbox::new(...)` / `Toggle::new(...)` | 点击或 Space/Enter 切换，返回 `changed()` |
| 搜索框 | `TextField::new(id, &mut text).hint(hint).icon(Icon::Search)` | 带搜索图标的输入字段，保留原生选择、剪贴板、IME |
| 输入字段 | `TextField::new(id, &mut text).label(...).validation(...)` | 标签、占位、帮助、校验、密码、只读、禁用；单行 Enter 保留焦点，Tab 继续导航 |
| 选择字段 | `SelectField::new(id, selected_text).show_ui(...)` | 全宽原生下拉框，统一字段高度、标签、帮助与校验；`native` 配置菜单 |
| 通用字段 | `Field::new(id).label(...).show(ui, control)` | 为原生或自绘控件提供标签、必填标记、帮助、校验及标签关联 |
| 富提示框 | `RichTooltip::new(&response, title).show(...)` | 延迟悬停或键盘聚焦显示，可组合说明、图标、属性；适配屏幕边缘 |
| 属性列表 | `properties(ui, &[Property::new(label, value)])` | 标签/数值对齐、长文本换行、窄宽度上下排列，支持数值强调色 |
| 滑块 | `egui::Slider::new(&mut value, range)` | 窄圆角拖动柄，保留原生拖拽与数值编辑 |
| 物品格 | `ItemSlot::new(label).icon(...).quantity(...).selected(...)` | 选中、数量、提示；`.image(TextureId)` 接入宿主图标 |
| 状态条 | `Meter::new(fraction).label("体力").color(...)` | 归一化进度，非有限值显示为空 |
| 输入提示 | `key_hint(ui, "A", "确认")` | 统一键帽外观，宿主决定设备与绑定 |
| 通知 | `notice(ui, NoticeKind::Success, text)` | 成功、警告、危险；颜色配合语义图标 |
| 图标 | `Icon::paint(painter, rect, color)` | 通用矢量图标与 14 类官方武器原图；武器绘制忽略 tint 保留原色 |

## 容器、布局与界面状态

| 能力 | API | 行为 |
| --- | --- | --- |
| 浮动窗口 | `Window::new(title).open(&mut open)` | 统一标题/关闭按钮；原生拖动、边缘缩放、位置与层级 |
| 滚动面板 | `ScrollPanel::new(id, title)` | 标题固定、内容裁剪；`show_rows` 虚拟化等高长列表；聚焦后按住方向键连续滚动 |
| 可交互虚拟列表 | `ScrollPanel::new(id, title).show_list(...)` | 按完整列表索引移动活动条目，自动滚动到屏外条目，跳过禁用项；支持上下、翻页、首尾导航 |
| 对话框 | `Dialog::new(id, title)` + `DialogState` | 模态遮罩、初始焦点、确认/取消、Esc、可配置点击背景关闭 |
| 弹出菜单 | `Popup::new(&anchor)` | 使用原生 Popup 开关状态，锚点点击切换、自动调整位置、点击外部关闭；内部通过 `ui.close()` 收起 |
| 响应式分栏 | `ResponsiveColumns::new(id).min_column_width(400.0)` | 等宽分栏、窄屏堆叠；默认最多两栏，重排时保留子控件 ID |
| 表单布局 | `FormLayout::new(id).show(ui, &fields, control)` | 共享标签列宽、同行标签高度与主控件起点；窄屏自动减少列数，左侧标签可移至上方 |
| 页签容器 | `Tabs::new(id)` + `NavigationState` + `Tab` | 选中内容作用域、禁用页签、左右/Home/End 导航；移除当前页后自动回退 |
| 导航栈 | `NavigationStack<Page>` | 压入页面、逐级返回、根页保护、返回后恢复入口焦点 |
| 通知队列 | `Notifications` | 轻量浮动提示、有界 FIFO、逐条展示、自动消失；默认鼠标穿透，调用方可覆盖 |
| 手柄 Focus Engagement | `FocusEngagement::begin/show/navigate` | 页面选择需要接入的区域；手柄方向选区、A 进入、B 返回，键鼠保持原生行为 |
| 方向导航 | `FocusGroup::vertical()` / `FocusGroup::grid(columns)` | 按行列移动、跳过禁用项、边界停留或循环、滚动至目标；不接管 Tab |
| 手柄动作适配 | `NavigationInput` + `GamepadState` | 四方向连发、确认/返回单次触发、前后焦点、最近输入设备、重绘调度 |

布局计算、尺寸协商、裁剪和层级仍交给 egui；库内统一外观与交互约定。行列、网格、分隔线、文字、图片、单选框和下拉框可以继续组合 egui 标准 API，继承主题。状态对象由宿主持有，核心不包含游戏业务、后台线程或系统输入采集。

控件焦点统一使用 egui 的 `Memory`：启用的按钮、选择行、复选框、开关和物品格在鼠标点击后请求同一个键盘焦点，随后可以直接用方向键或 Enter 操作。悬停不会抢焦点，禁用组件不会因点击获得焦点。

`consume_escape(ctx)` 在子内容处理完输入后消费剩余的无修饰键 Esc，只对新的按下返回 `true`，丢弃长按连发。文本字段先沿用 egui 的 Esc 失焦行为并消费该次按键，保留已编辑文本；导航栈和对话框随后才处理返回或关闭。返回后的父页面及焦点在下一次绘制时恢复。这个函数不选择活动界面，也不决定是否拦截游戏输入：宿主应在当前活动界面的子弹层处理完后调用它，并根据界面是否仍接管输入设置宿主策略，不能仅凭 `egui_wants_keyboard_input()` 判断界面已经退出。

```rust
// 每帧根据可用宽度排列三个面板；id 在整个 egui Context 内唯一。
ResponsiveColumns::new(egui::Id::new("overview"))
    .min_column_width(400.0)
    .max_columns(3)
    .show(ui, 3, |ui, index| {
        Panel::new(["装备", "道具", "委托"][index]).show(ui, |ui| {
            ui.label("面板内容");
        });
    });

let mut panel = ScrollPanel::new(egui::Id::new("archive"), "档案");
panel.scroll = panel.scroll.max_height(260.0);
panel.show_rows(ui, 36.0, 10_000, |ui, visible_rows| {
    for row in visible_rows {
        ui.add_sized([ui.available_width(), 36.0],
            egui::Label::new(format!("档案 {row}")));
    }
});
```

`show_rows` 的行高不包含行间距，调用方应保持每行等高。分栏中的索引代表稳定位置；如果数据会排序或增删，内容控件另用稳定业务 ID。`ScrollPanel` 的滚动 ID 相对于父 UI；分栏、导航页面和页签内容提供稳定父作用域。

`show` / `show_rows` 为滚动内容创建原生 `UiBuilder::sense` 焦点入口，可通过 Tab 或点击内容空白处聚焦。聚焦滚动视口时，按住 Up/Down 连续滚动，PageUp/PageDown 翻页；焦点在子文本框或按钮上时保留子控件行为。标题和外层 Panel 只负责展示。已有的原生 `egui::ScrollArea` 可在内容闭包末尾调用 `scroll_keyboard(ui, focus_id)` 接入相同行为，调用处以 `UiBuilder::sense` 为滚动视口提供焦点入口。内层滚动区独立消费自己的按键，点击内层空白处也不会被外层抢走焦点。

如果每行代表一个选择项，使用 `show_list`。整个列表只有一个原生焦点；Up/Down、PageUp/PageDown、Home/End 改变内部活动索引，并通过原生 ScrollArea 滚动到该条目。键盘 Tab / Shift+Tab 始终离开列表，继续遍历页面控件；选中值由宿主保存，方向导航不会提交选择。手柄进入区域后才操作列表内部，B 返回区域层。

```rust
let mut panel = ScrollPanel::new(egui::Id::new("quest-list"), "委托档案");
panel.scroll = panel.scroll.max_height(258.0);
let list = panel.show_list(ui, 36.0, 10_000, |_| true, |ui, row| {
    ui.add_sized([ui.available_width(), 36.0],
        Button::new(&format!("第 {:05} 号委托", row + 1))
            .sense(egui::Sense::CLICK))
});
if let Some(row) = list.inner.activated {
    // 鼠标点击和键盘确认统一在这里查阅委托。
}
```

第一个闭包查询某行是否启用，第二个闭包只渲染可见行并返回鼠标响应。行使用 `Sense::CLICK`，不能注册独立键盘焦点；`Sense::click()` 同时包含 `FOCUSABLE`，适用于普通按钮。包含文本框、按钮等多个独立操作控件的表单使用原生 ScrollArea 或 `show_rows`，保留这些控件的原生 Tab 顺序。

组件通过原生 `Response::gained_focus()` / `scroll_to_me(None)` 在 Tab 切换时显示目标，滚动距离与动画由 egui 计算；不会持续锁定滚动位置。直接组合原生控件时，可在滚动内容闭包内调用 `scroll_on_focus(&response)` 接入同一行为。

列表获得焦点后恢复活动条目，Enter / Space 直接提交选择；鼠标点击列表空白处聚焦列表，点击条目直接选择。标题和边框保持展示职责。手柄的 A 先进入登记的区域，该次按键不会同时提交列表选择，后续 A 才确认；B 从内部返回区域层。长按手柄确认不会重复提交选择。

`show_list` 返回 `InnerResponse<ListOutput>`：外层 `response` 描述展示面板，`inner.response` 是列表控件的原生焦点响应。`inner.scroll` 是 `ScrollAreaOutput`，`inner.active` 是记忆的活动索引，`inner.activated` 是本帧点击或确认的索引，`inner.active_response` 提供已渲染活动条目的几何信息。条目不是独立的 egui 键盘焦点；辅助技术通过原生 AccessKit 的 ListBox / ListBoxOption 和 active descendant 获取活动项。普通 `show` / `show_rows` 仍返回 `InnerResponse<ScrollAreaOutput<_>>`。

```rust
use egui_hunter::DialogState;

// 放在宿主状态里，每个对话框一个实例，不要每帧重新创建。
let mut dialog = DialogState::default();

// 每帧：先画入口，再画对话框。
let opener = ui.add(Button::new("打开确认"));
if opener.clicked() {
    dialog.open_from(&opener);
}
let confirm = egui::Id::new("confirm-action");
Dialog::new(egui::Id::new("confirmation"), "确认操作")
    .initial_focus(confirm)
    .show(ui.ctx(), &mut dialog, |ui| {
        if ui.add(Button::new("确认").id(confirm)).clicked() {
            ui.close();
        }
    });
```

即使对话框已关闭，也继续调用 `show`，使入口重新渲染后能恢复焦点；宿主也可调用 `DialogState::open/close`。默认关闭判断复用原生 `ModalResponse::should_close()`；`.dismiss_on_backdrop(false)` 只保留 Esc 和 `ui.close()` 关闭。

需要独占交互的页面在最外层使用原生 `Modal`，内部正常组合 Panel、列表和表单。Modal 隔离背景交互，`FocusEngagement` 只负责可选的手柄区域操作；不为每个面板创建 Modal。页面持有开关和业务状态，公共 `DialogInteraction` 负责打开、关闭及入口焦点恢复，键盘初始焦点可直接设到列表或按钮，手柄初始区域交给 `FocusEngagement::begin`。普通浮窗继续使用 `Window`。子 Modal 和 Popup 优先处理输入。

```rust
use egui_hunter::components::dialog::interaction::DialogInteraction;

// page_state 由页面持有，打开时调用 open(ctx) 或 open_from(&opener)。
let action = egui::Id::new("page-action");
DialogInteraction::default()
    .initial_focus(action)
    .dismiss_on_backdrop(false)
    .show(ctx, &mut page_state, egui::Modal::new(egui::Id::new("page")), |ui| {
        Panel::new("委托详情").show(ui, |ui| {
            if ui.add(Button::new("完成").id(action)).clicked() {
                ui.close();
            }
        });
    });
```

原生 Modal 提供背景交互隔离和模态层优先级，`UiBuilder::closable()` / `ui.close()` 表达向所属容器请求关闭，`EventFilter` 让当前控件保留特定按键。手柄 Engagement 独立于这些生命周期能力，不增加通用 `FocusScope`。Modal 只隔离 egui 内部输入，游戏输入仍由 Overlay 宿主仲裁：需要独占输入的界面显示时阻断鼠标和键盘，关闭后恢复穿透。

```rust
let anchor = ui.add(Button::new("营地行动"));
Popup::new(&anchor).title("营地行动").show(|ui| {
    if ui.add(Button::new("补充道具")).clicked() {
        ui.close();
    }
});
```

Popup 不需要宿主持有 `DialogState`。开关状态由 egui memory 唯一管理，打开另一个菜单会立即关闭旧菜单。默认 ID 为 `egui::Popup::default_response_id(&anchor)`，也可用 `popup.native = popup.native.id(id)` 指定；程序控制使用原生 `egui::Popup::open_id/close_id/is_id_open`。每帧调用 `show`，包括关闭后的下一帧，以便恢复入口焦点；外部点击保留新位置的焦点，父界面隐藏后不会补发过期的恢复请求。

页面键需实现 `Debug + Eq + Hash`。`NavigationStack::show` 渲染当前页，入口响应传给 `push_from(&response, next_page)`；返回按钮或手柄 B 动作调用 `back(ctx)`。在 `show` 之后提交的导航下一帧生效。每帧按主界面、外层对话框、内层对话框顺序渲染；Esc 优先关闭当前弹出菜单/顶层对话框，然后才返回上级页面。多个界面同时可见时，宿主应只对当前活动界面启用输入。

`Notifications::new(id)` 默认最多 32 条，一次显示一条；`push(ctx, kind, text)` 默认显示 3 秒，`push_for` 自定义时长。**排队中的消息从首次展示开始计时**。达到容量时移除最早等待的消息，保留当前可见消息；容量为 1 时替换当前消息。通过返回 ID 调用 `dismiss`，或调用 `clear`。主界面绘制后调用 `notices.show(ctx)`。

短暂的成功、提醒消息使用 `Notifications`：只显示图标和文字，没有关闭按钮，默认允许鼠标穿透。
调用方可以在 `show` 前使用 `notices.set_pass_through(false)` 阻止点击到达后面的 egui 控件，
也可以设置为 `true` 恢复穿透；这不会增加关闭按钮或改变消息时长。游戏是否收到输入由宿主输入策略决定。
需要持续展示的状态使用页面内的 `notice` 或字段校验；需要用户确认、取消或作出选择的操作使用 `Dialog`。

## 信息组件与输入状态

```rust
use egui_hunter::{Icon, ItemSlot, Property, RichTooltip, TextField, Validation, properties};

let validation = if name.trim().is_empty() {
    Validation::Error("请填写猎人姓名")
} else {
    Validation::Success("姓名可以使用")
};
let response = ui.add(TextField::new(egui::Id::new("name"), &mut name)
    .label("猎人姓名").hint("输入姓名")
    .help("可使用中文").validation(validation));

let item = ui.add(ItemSlot::new("铁刀").icon(Icon::Sword).hover_text(false));
RichTooltip::new(&item, "铁刀 · 锻造资料").show(|ui| {
    ui.label("工坊以精炼矿石打造的武器。");
    properties(ui, &[
        Property::new("攻击力", "528 (+48)").color(egui_hunter::Tokens::get(ui).success),
        Property::new("锻造费用", "2400 z"),
    ]);
});
```

字段返回原生编辑器的 `Response`，标签和帮助文本不会改变显式 ID。校验由调用方决定，校验消息优先于帮助文本，并同时显示文字、颜色和图标。`.read_only(true)` 保留文本选择与复制；`.password(true)` 继承 egui 的遮掩和禁止复制行为；`ui.add_enabled(false, field)` 禁止交互，也会移除原有编辑焦点。搜索框直接组合 `TextField::new(...).icon(Icon::Search)`。

富提示框沿用 egui 的悬停延迟与定位，不开启菜单状态，也不主动改变焦点。禁用控件也可附上原因说明；`.on_focus(false)` 可关闭聚焦展示。给物品格附加富提示框时用 `.hover_text(false)` 关闭默认的纯文字提示。属性列表继承父面板的文字颜色；单位、差值和本地化文案由宿主格式化。极长内容应由调用方在提示框内组合滚动区域。

### 表单字段与对齐

`Field` 负责一个字段的标签、必填标记和说明；`FormLayout` 负责字段之间的列宽、行高与响应式布局。表单先测量标签文字，再绘制一次控件闭包，不通过重复运行控件测量尺寸。校验规则、提交和保存状态仍由宿主提供，`.required(true)` 只表达必填标记。

```rust
use egui_hunter::{Field, FormLayout, LabelPlacement, SelectField, TextField, Validation};

let name_id = egui::Id::new("profile-name");
let camp_id = egui::Id::new("profile-camp");
let fields = [
    Field::new(name_id).label("猎人姓名").required(true)
        .validation(if name.trim().is_empty() {
            Validation::Error("请填写猎人姓名")
        } else {
            Validation::None
        }),
    Field::new(camp_id).label("集合地点").help("选择队伍集合的营地"),
];
FormLayout::new(egui::Id::new("profile-form"))
    .max_columns(2)
    .min_column_width(280.0)
    .label_placement(LabelPlacement::Left)
    .label_align(egui::Align::Max)
    .show(ui, &fields, |ui, index| match index {
        0 => ui.add(TextField::new(name_id, &mut name)),
        _ => SelectField::new(camp_id, camps[camp])
            .show_ui(ui, |ui| {
                for (index, label) in camps.iter().enumerate() {
                    if ui.selectable_value(&mut camp, index, *label).clicked() {
                        ui.close();
                    }
                }
            }).response,
    });
```

默认单列、标签在上。`.label_placement(LabelPlacement::Left)` 使用共同的标签列宽，`.label_width(...)` 可显式指定，`.label_align(...)` 控制标签文字在列内的左右位置。左侧标签默认对齐主控件中心；多行编辑器等高控件可在对应 `Field` 上设置 `.label_vertical_align(egui::Align::Min)`，让标签靠顶。可用宽度不足时，布局自动改为上方标签，避免标签挤占编辑区域。

上方标签允许换行，同一行预留相同标签高度；没有标签的字段也保留该行标签槽。帮助和校验文字位于各自控件下方，不参与主控件居中，下一行按本行最大高度排列。字段作用域将原生 `interact_size.y` 下限设为 40 点，`TextField` 和 `SelectField` 使用该尺寸，并继承更大的局部样式；其他控件仍可通过自身 API 配置尺寸。复合字段可在闭包中组合输入框、单位与按钮，返回主控件的 `Response`，让标签关联和垂直对齐使用这个控件。

`FormLayout` 使用字段的稳定 ID 建立作用域，调整宽度或字段顺序不会按列号重建原生控件身份。字段内部使用 `TextField`、`SelectField` 时只传控件内容，标签和校验统一放在外层 `Field` 上。布局不会增加 Tab 停靠点。独立使用时，两个字段控件也可继续直接调用 `.label(...)`、`.help(...)`、`.validation(...)`。

`SelectField.native` 保留原生 `ComboBox` 配置，例如菜单高度和关闭策略。`show_ui` 返回原生的菜单结果和控件 `Response`；业务在菜单内处理 `selectable_value(...).changed()` 等选择事件，不根据打开菜单推断数据已改变。需要确认选项后收起时，在选项 `.clicked()` 分支调用 `ui.close()`，使鼠标和键盘确认都关闭菜单，包括确认当前已选项。关闭后复用 `Popup` 的入口焦点恢复规则；外部点击保留新位置的焦点。已打开的字段被禁用时，菜单关闭且不再执行选项闭包。

## 方向与手柄导航

普通按钮直接使用 egui 原生方向导航即可，无需注册 FocusGroup。只有需要严格行列、边界停留或循环时才使用下面的分组规则：

```rust
use egui_hunter::FocusGroup;

let controls = [
    ui.add(Button::new("森林探索")),
    ui.add_enabled(false, Button::new("未解锁")),
    ui.add(Button::new("采集与调合")),
];
FocusGroup::vertical().wrap(true).navigate(ui, &controls);
```

每帧先渲染再传入同组的 `Response`，网格按行优先排列，禁用项也要保留原位置。焦点和选中值独立：方向键只移动焦点，Space/Enter 确认才修改宿主状态。默认边界停留，`.wrap(true)` 在同一行/列循环；不完整末行不会跳到其他列。单列组只接管 Up/Down，Left/Right 继续使用原生几何导航；分组不接管键盘 Tab/Shift-Tab，保留页面控件的原生顺序。只注册需要这些约束的控件，文本编辑器和滑块继续处理自己的方向键。可交互虚拟列表使用 `ScrollPanel::show_list`，它会处理尚未渲染行的导航。

鼠标导航按钮可以调用 `FocusGroup::move_focus(ui, &controls, from_id, direction)`，无需伪造手柄输入。宿主应独立保存导航位置和已选值，用显式的起点移动目标控件的焦点；鼠标导航不应伪造手柄事件来改变设备提示。

页面可选择使用 `FocusEngagement` 为密集列表、表单等区域提供手柄进入／退出交互。它参考 [Microsoft 的 Xbox/UWP Focus Engagement](https://learn.microsoft.com/en-us/windows/uwp/ui-input/gamepad-and-remote-interactions#focus-engagement)：方向先选择整个区域，A 进入并恢复内部目标，B 退出。官方指南明确该模式不影响键盘及其他输入设备；本库同样只对经过宿主适配的手柄动作生效。

| 输入设备 | 导航与操作 |
| --- | --- |
| 键盘 | Tab / Shift+Tab 按原生顺序遍历页面实际控件，列表是一站；Enter / Space 直接操作当前控件，Esc 按原有编辑、弹层、页面顺序处理。 |
| 鼠标 | 单次点击直接操作控件，悬停和滚轮沿用 egui；面板背景只负责展示。 |
| 手柄 | 未进入时方向键选择登记的区域，A 进入；进入后操作内部控件，B 在子编辑或弹层处理之后退出区域，再次 B 交给页面。RB / LB 在区域层切区，进入后切内部控件。 |

键鼠操作不会要求先确认进入框，也不会禁用其他区域的控件或移除页签头、页脚的焦点。仅手柄会把登记区域作为导航目标；隐藏或禁用的记忆目标回退到首个可用控件，没有可操作控件的区域会被跳过。确认进入的按键不会同时激活控件，长按手柄确认或取消不会连续跨层。

每帧先 `begin`，再用 `show` 登记各区域及其原生控件 `Response`，最后 `navigate`。`begin` 放在页面相关内容前，`navigate` 放在共同的 ScrollArea 内容闭包内，以便显示需要恢复的目标。`show` 不绘制面板，也不要求关闭未进入区域的交互；只有业务上禁用的控件才使用 `ui.add_enabled(false, ...)`。

```rust
use egui_hunter::FocusEngagement;

let mut engagement = FocusEngagement::new(egui::Id::new("settings-engagement"));
// initial_region 仅在页面打开时指定；它是手柄目标，不替代键盘初始焦点。
engagement.begin(ui, initial_region);
let region = egui::Id::new("hunter-region");
engagement.show(ui, region, |ui, controls| {
    // 页面将手柄区域焦点映射为原生样式，Panel 只负责绘制。
    if ui.memory(|memory| memory.has_focus(region)) {
        let active = ui.visuals().widgets.active;
        ui.visuals_mut().window_fill = active.bg_fill;
        ui.visuals_mut().window_stroke = active.bg_stroke;
    }
    Panel::new("猎人登记").show(ui, |ui| {
        controls.push(ui.text_edit_singleline(&mut hunter_name));
        let confirm = ui.add(Button::new("确认登记"));
        if confirm.clicked() {
            // 提交仍由宿主处理。
        }
        controls.push(confirm);
    });
});
// 列表区域把 list.inner.response 放入 controls，列表条目不用逐一登记。
engagement.navigate(ui);
```

手柄来源必须在转换成 egui Key 之前保留并路由，不能根据收到的 Tab、Enter 或 Esc 猜测设备。启用手柄的宿主先安装插件，再把采集到的 `GamepadState` 交给 `NavigationInput`；插件将手柄 Engagement 动作与物理键盘事件分开处理。没有登记区域时，手柄适配继续提供普通 egui 导航，RB / LB 回退为 Tab / Shift+Tab。

```rust
use egui_hunter::{Direction, EngagementPlugin, GamepadState, NavigationInput};

// 手柄宿主初始化一次；Theme::apply 只安装样式。
context.add_plugin(EngagementPlugin::default());
// 每个宿主输入流持有一个实例。
let mut navigation = NavigationInput::default();

// 每帧在 Context::run/run_ui 前调用；eframe 使用 App::raw_input_hook。
// 这里的状态来自真实手柄采集，物理键盘事件保持在 raw_input 中。
navigation.apply(&context, &mut raw_input, GamepadState {
    direction: Some(Direction::Down),
    confirm: false,
    cancel: false,
    next_focus: false,     // RB：手柄当前层的下一个目标
    previous_focus: false, // LB：手柄当前层的上一个目标
});
```

Focus Engagement 仅在根 `egui::ViewportId::ROOT` 生效。同一根 viewport 内的原生 Window、Modal 和 Popup 正常参与交互；额外的原生操作系统 viewport 完全回退为原生导航，不读写根 viewport 的 Engagement 状态。`NavigationInput` 仍可用于这些输入流的普通手柄按键适配。

`FocusEngagement::begin` 也会确保插件已安装；宿主在首次转换手柄输入前显式安装，可以从第一帧保留动作来源。方向首次按下立即触发，默认延迟 350 ms 后每 90 ms 连发，`repeat_timing` 可调整。其余动作仅在按下边沿触发；松开或断开设备时传默认状态。窗口失焦期间忽略动作，恢复窗口不会重新触发仍按住的确认键；合成按键不会释放真实键盘上仍按住的同名键。`NavigationInput::device()` 返回最近主动操作的输入设备，用于选择实际键帽提示；仅鼠标悬停移动不切换手柄提示。同一帧存在物理键鼠操作时优先交给键鼠，仍按住的手柄方向需松开或改变后才重新接管。

系统手柄采集、摇杆死区和游戏/UI 输入仲裁仍由宿主完成。组件展厅没有采集真实手柄，其键帽选项只预览展示样式，不能用物理键盘事件伪装手柄来验证设备兼容性。

## 交互约定

- **选中**：深金色填充 + 金色勾选标记，失去焦点后仍保留。
- **聚焦**：按钮、选择控件和文本框只加粗原有内部边框；手柄选区时页面强调区域边框，进入后强调内部控件。键盘和鼠标只突出当前控件，列表突出活动条目，不增加布局占位。
- **悬停 / 按下**：沿用原生 hovered / active 状态，预设保持普通结构边框；内容在按下时保持原位。
- **主要动作**：金色填充与深色文字；**危险 / 校验**：颜色配合警告图标和文字，不替换焦点标记。禁用统一使用 `ui.add_enabled(false, widget)`。
- 自绘交互组件注册 egui 的可访问性元信息，沿用 egui 的点击、Tab、Space/Enter 和禁用行为。
- 动态列表使用 `ui.push_id(stable_item_id, ...)`，使筛选和排序后的控件身份保持稳定。
- 坐标均为 egui 逻辑点，宿主负责 DPI/缩放；默认采用横向 8 点、纵向 6 点、分组 12 点间距，36 点最小操作高度。虚拟列表的行高须匹配控件实际高度，包括字体、图标和内边距。
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

示例优先使用 `--font`，否则尝试常见的系统中文字体；字体不会打包进库。`--compact` 只切换窄窗口，`--density compact` 切换紧凑控件（默认 `--density standard`）；两项可以组合使用。`--dialog` 打开任务确认弹窗。`--containers` 直接进入容器与导航页，`--popup` / `--window` 同时打开该页的弹出菜单 / 浮动窗口，`--notices` 播放消息队列。`--details` 进入信息与交互页，`--form-labels-left` 同时将登记表切换为左侧标签，`--tooltip` 同时聚焦装备图标以展示富提示框，便于截图检查。

组件页包含任务确认、背包分类/搜索/整理、道具消耗与体力更新、禁用状态、键鼠/手柄提示和 HUD。容器页包含三级营地菜单、一万条委托档案、浮动手记、行动菜单、确认对话框和消息队列；键盘 Tab 原生遍历控件，Enter 直接操作，Esc 关闭弹层或返回菜单。页面保留可选的手柄区域声明，需由采集手柄输入的宿主接入。信息页的登记表组合文本、密码和下拉字段，可切换标签在上或在左以检查响应式对齐；其余区域包含装备属性与富提示框、装备选择和只读/禁用/校验状态。装备选择下方的向上/向下按钮直接移动焦点并支持按住连发，确认后切换装备。示例未采集真实手柄。窄窗口自动堆叠面板并允许纵向滚动。

## 验证

```sh
cargo test -p egui-hunter --all-targets --target aarch64-apple-darwin
cargo test -p egui-hunter --doc --target aarch64-apple-darwin
cargo clippy -p egui-hunter --all-targets --target aarch64-apple-darwin -- -D warnings
cargo check -p egui-hunter --lib --target i686-pc-windows-msvc
```

测试通过合成 egui 输入检查点击、键盘、禁用、中文编辑、只读复制与密码限制、方向边界/连发/焦点切换、提示框延迟/定位、窗口移动/缩放/关闭、浮层关闭顺序与焦点恢复、长列表虚拟化和滚动位置、响应式 ID、页签导航及通知生命周期。截图来自真实 eframe/egui 渲染；系统 IME、真实手柄和实际游戏内输入仍需在对应宿主验证。
