# mhf-game

固定 HD 客户端 `mhfo-hd.dll` 的通用 Windows x86 游戏内核，负责启动内存、游戏 DLL、
Mod 生命周期和最终退出。它不依赖具体 Mod、配置 schema、Sign 客户端或界面实现。
应用完成配置与包选择，再注入内置 Factory。

## 入口与职责

`LaunchConfig` 包含游戏目录、预先映射的 `MhfLaunchParams32` 和 `RuntimeConfig`。
`run(&config, &profile, resolved, factory)` 运行统一生命周期；`GameExit` 包含可选游戏退出码与 Mod 状态，
退出码为 `None` 表示启动提供方取消。固定客户端描述在 `runtime::PROFILE`，类型为 `MhfLaunchProfile`。

| 位置 | 职责 |
| --- | --- |
| [`src/game/mod.rs`](src/game/mod.rs) | 统一准备、启动、运行与清理 |
| [`src/game/native.rs`](src/game/native.rs) / [`src/abi.rs`](src/abi.rs) | 游戏 DLL、主入口、宿主内存和 Win32 句柄 |
| [`src/runtime/mod.rs`](src/runtime/mod.rs) | 通用 LaunchConfig 与固定 PROFILE |
| [`src/profile.rs`](src/profile.rs) | 客户端描述 |

应用的 [`runtime.rs`](../../apps/launcher/src/runtime.rs) 读取配置并解析目录，
[`builtins.rs`](../../apps/launcher/src/builtins.rs) 组装具体 Mod。内置清单和 semver 解析属于
[`mhf-mod-package`](../mod-package/README.md)，实例、接口和阶段由 [`mhf-mod-host`](../mod-host/README.md) 管理。

## 启动与退出

prepare 完成后，宿主优先选择 `mhf.launch.v1`；没有普通提供方时使用 `mhf.launch.fallback.v1`。
有效层必须恰好有一个提供方。回调借用初始化后的 `LaunchParams32` 和 `GlobalData32`，
成功后才加载游戏 DLL、执行 check/attach 并进入 `mhDLL_Main`；取消走正常清理。
默认 Login 和 Debug 都实现这份契约，内核没有 Login／Debug 分支。

内存和句柄始终由内核持有。Mod 不得保留启动回调的目标指针，也不得修改宿主句柄和指针字段。
内置 Module 的 prepare_release 归还额外游戏 DLL 引用，退役缓冲保留到游戏卸载完成；
清理失败时保留相关 Mod、DLL 与内存。完整契约见 [Mod 系统](../../../docs/mod-system.md)。

## 共享配置与资源

配置存储与通用 INI 桥由独立 [`mhf.config`](../../mods/config/README.md) 提供。
Base 提交游戏设置及 INI 映射，Login 自己注册并解析所属配置；内核只接收应用准备的值。
字体、界面、Geometry 与任务字节属于 Base 内部组件；Unicode 和 Translation crate 当前保留但不接入应用。

任务与调试说明见 [Debug](../../mods/debug/README.md)，登录编码与 ABI 字段规则见
[Login](../../mods/login/README.md)，界面和输入法见 [UI](../../mods/ui/README.md)。

## 构建与验证

在 `mhf/` workspace 运行 `cargo check -p mhf-game --all-targets --target i686-pc-windows-msvc`。
实际运行使用唯一的 `mhf-launcher`，构建与 Nix／Wine 用法见 [启动器](../../apps/launcher/README.md)。

[`src/game/tests.rs`](src/game/tests.rs) 检查最小启动提供方注入和真实 HD DLL 的加载／卸载，
不调用游戏主入口。具体 Mod 的 Hook、UI、任务和资源逻辑由其组件测试覆盖，游戏交互仍需单独验收。

C 头文件由 [`apps/launcher/build.rs`](../../apps/launcher/build.rs) 聚合，
[`apps/launcher/tests/headers.rs`](../../apps/launcher/tests/headers.rs) 校验快照；内核不承担领域构建依赖。
完整命令见 [头文件生成](../../../docs/dll-mods.md#头文件生成)。
