# MHF 工作区

固定 HD 客户端 `mhfo-hd.dll` 的游戏宿主、功能 Mod 与启动应用。Rust crate 统一放在 `crates/`，
按领域和职责分为四组：

| 目录 | 内容 |
| --- | --- |
| [`crates/mods/`](crates/mods/) | Config、Base、Login、Debug、Workbench 运行 Mod，Font／UI／Quest／Geometry／Monster 组件，以及暂未接入的 Unicode／Translation crate |
| [`crates/runtime/`](crates/runtime/) | 游戏会话、Mod 生命周期、公共 C 协议、Rust SDK、Hook 管理、包发现与依赖解析 |
| [`crates/shared/`](crates/shared/) | [`egui-hunter`](crates/shared/egui-hunter/README.md) 组件与 [`resource`](crates/shared/resource/README.md) 游戏资源类型 |
| [`crates/apps/`](crates/apps/) | 唯一游戏入口 [`launcher`](crates/apps/launcher/README.md) 及独立管理器 [`mod-manager`](crates/apps/mod-manager/README.md) |

## 领域 API 与提供方

Config、Font、UI、Quest、DebugTools 的 API 是当前公开 Rust 接口与 C 布局的唯一来源。
crate 默认提供 API，`provider` feature 增加该领域的 Service、内置 `Module`、资源或原生后端。
`headers` feature 开启头文件定义函数，供 [`apps/launcher/build.rs`](crates/apps/launcher/build.rs)
聚合到 `OUT_DIR/include`。API 构建不依赖 provider 实现。

Cargo 会合并 features；API 与 provider 可以同时启用，provider 在 API 基础上增加实现。
默认 API 层不依赖 egui、原生 Hook 或 ModHost。检查这条边界时使用独立消费者工程，
避免工作区内其他 provider 使用方的 feature 合并影响依赖图。

| 领域 | 入口 |
| --- | --- |
| 配置与 INI 桥 | [`mhf-config`](crates/mods/config/README.md) |
| 字体 | [`mhf-font`](crates/mods/font/README.md) |
| 面板、控件与 D3D9 UI 后端 | [`mhf-ui`](crates/mods/ui/README.md) |
| Unicode 文本与原生 IME（暂未接入） | [`mhf-unicode`](crates/mods/unicode/README.md) |
| 译文与词典（暂未接入） | [`mhf-translation`](crates/mods/translation/README.md) |
| 当前离线任务后端 | [`mhf-quest`](crates/mods/quest/README.md) |
| 游戏线程调试工具 | [`mhf-debug-tools`](crates/mods/debug-tools/README.md) |
| 资源浏览与原生模型工作台 | [`mhf-workbench`](crates/mods/workbench/README.md) |
| 几何扩展 | [`mhf-geometry`](crates/mods/geometry/README.md) |
| 怪物种类上限补丁 | [`mhf-monster`](crates/mods/monster/README.md) |

[`BuiltinCatalog`](crates/runtime/mod-package/src/profile.rs) 提供启动应用与管理器共用的内置清单。
应用的 [`builtins.rs`](crates/apps/launcher/src/builtins.rs) 组装 Factory；通用游戏宿主只接收已解析的 Mod。
运行清单包括 `mhf.config`、`mhf.base`、`mhf.login`、`mhf.debug`、`mhf.workbench`。
Config 独立提供通用配置注册、存储与 INI 映射，不依赖消费者 schema。
Base 统一 Font、UI、Geometry、Monster 和 Quest 的生命周期，提供字体、界面及任务能力。Quest 只在启动 Mod 请求本地会话后安装任务 Hook。
组件 crate 保留各自 API 与实现，不单独参与 Mod 选择。
Base 提交游戏字段与 INI 映射，Login 自己注册并解析 Sign 配置。游戏资源和任务保留原始文本。

Login 是默认 fallback 启动提供方；启用 Debug 或 Workbench 自动覆盖登录流程。启动 Mod 通过公开启动接口填充宿主借用的
固定游戏缓冲区。配置示例见 [启动器](crates/apps/launcher/README.md#启动选择与配置)。

内置 `mhf_mod_host::Module` 包含 `prepare_release`，用于释放额外游戏 DLL 引用并保留退役缓冲。
外部 DLL 使用的 `mhf_mod_sdk::Mod` 和 C 生命周期没有该阶段；独立 DLL 入口使用 `export_mod!`
时，仍需遵守外部 Mod 的 detach 与 DLL 所有权契约。

## 构建与文档

游戏宿主及启动器目标为 `i686-pc-windows-msvc`。在配置好相应工具链的环境中，从仓库根运行：

```sh
cargo check --manifest-path mhf/Cargo.toml -p mhf-game --all-features --all-targets --target i686-pc-windows-msvc
cargo build --manifest-path mhf/Cargo.toml -p mhf-launcher --bin mhf-launcher --release --target i686-pc-windows-msvc
cargo build --manifest-path mhf/Cargo.toml -p mhf-mod-manager --release --target i686-pc-windows-msvc
```

在项目 Nix 开发环境中，`mhf-mods` 构建并打开独立管理界面，`mhf-mods list` 保留命令行用法。
管理器默认读取调用目录的 `mhf.toml`，路径可用启动参数覆盖；界面检查明确启用项的依赖，保存设置、导入或导出 ZIP；
它不加载游戏或 Mod DLL。路径和构建能力设置见 [管理器文档](crates/apps/mod-manager/README.md)。

目录和代码组织见 [Mod 系统](docs/mod-system.md)，包、接口、头文件检查与导出见
[DLL Mod](docs/dll-mods.md)，原生排空和目标冲突见 [Hook](docs/mod-hooks.md)。
C／Rust DLL smoke 检查跨 DLL 调用，HD DLL 测试检查安装、恢复和最终释放；游戏主循环中的交互仍需单独验收。
