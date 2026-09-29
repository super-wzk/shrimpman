# mhf-font

默认导出 [`api`](src/api/mod.rs) 和同名根入口：`Font` 绑定 `mhf.font.v1`，
`FontApi`／`FontTable` 描述同一份公开接口，`FAMILY_NAME` 提供默认字体名。
接口由 `mhf.base` 发布；消费者声明对 Base 的依赖。
`family()` 借用提供方持有的稳定 UTF-8 字符串，不复制缓冲或转移所有权。
API 不依赖字体资源、egui 或原生 Hook；`headers` feature 开启 `api::define_header`，
供启动应用构建脚本聚合 C 头。

`provider` feature 提供内嵌字体和 `install(&egui::Context)`；Windows 上的 `FontMod` 使用内部 `FontService` 发布接口，并由 [`mhf-base`](../base/README.md) 组合运行。
字体文件只保存在 [`assets`](assets/JetBrainsMapleMono-NF-XX-NL-HT-Regular.ttf)，
启动器和游戏界面通过同一安装函数使用它。

Windows provider 导出供 Base 组合的 `FontMod`，实现内部 `Module` 生命周期：

- `FontMod::new(name, renderer)` 接受字体名和 `Option<TextRenderer>`。
- prepare 按所选名称注册进程内字体，并发布 `FontService` 的稳定接口表。
- attach 安装字体创建、度量、绘制和单字图集 DPI Hook；detach 排空并移除 Hook，失败保留状态供重试。
- 字体注册与退役状态随提供方实例销毁，处于宿主的完整退出顺序内。

`TextRenderer` 保留为可选扩展接口；当前 Base 传入 `None`，使用原生文本处理。`corrected_y` 供使用 W API 的原生文本绘制共享字体基线修正；
`install_game` 和 `HookState` 供原生集成及其测试使用。提供方不定义独立 DLL 导出入口。

原生 32×32 单字位图每次完整清除背景，并在复制像素前刷新 GDI，避免残留上一字的笔画。
单字图集的字号计算固定采用 96 DPI，即 `-MulDiv(22, 96, 72) = -29`，字体名称、字重和质量仍按配置生效。
该指令 Hook 仅支持已验证的 ZZ HD 客户端，在 RVA `0x014D3325` 校验原始指令后安装，
随字体 Hook 组一起卸载；不修改全局 DPI 查询或进程 DPI 设置。
直接调用 `install_game` 仅安装 GDI Hook；`FontMod::attach` 额外绑定宿主提供的游戏模块并安装 DPI Hook。
