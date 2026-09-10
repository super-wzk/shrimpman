# MHF launcher

`mhf-launcher` 是唯一可执行启动入口。应用发现 Mod、解析选择并组装内置提供方，
[`mhf-game`](../../runtime/game/README.md) 负责通用游戏 ABI、生命周期和退出。
登录由 [`mhf.login`](../../mods/login/README.md) 提供，临时猎人及调试工具由
[`mhf.debug`](../../mods/debug/README.md) 提供；启动器没有另一套运行模式设置。

[`mhf.base`](../../mods/base/README.md) 汇集字体、UI、Geometry 和 Quest，
独立 [`mhf.config`](../../mods/config/README.md) 提供配置与通用 INI 桥。
Quest 是 Base 的内部组件，普通 Login 不激活本地任务 Hook。准备阶段结束后，宿主优先选择普通启动提供方；
没有普通提供方时使用 fallback。有效层必须恰好有一个提供方，否则在启动回调前报错。取消登录正常结束启动。

独立管理器 [`mhf-mods`](../mod-manager/README.md) 默认打开图形界面，编辑同一份配置的 Mod 开关和版本要求。
它按启动组合诊断依赖，保存和导出时解析明确启用项及其声明依赖；启动器按默认项与声明依赖解析选择。
`--list-mods` 显示启动器的解析选择，`--export-modpack` 导出该组合，两者均不运行启动回调或游戏。

## 启动选择与配置

默认构建包含 `login`、`debug`，自动选择 Login，Base 由 Login、Debug 等 Mod 的声明依赖按需带入。
调试使用同一个 `mhf-launcher`，只需在 `mhf.toml` 中调整 Mod 选择：

```toml
[mods."mhf.debug"]
enabled = true

# 可选；省略时使用内置古迹任务。
[mods."mhf.debug".settings]
quest = "quests/test.bin"
```

Debug 的普通启动接口自动覆盖 Login 的 fallback。任务相对路径以 Debug 的资源目录
（内置时为启动器可执行文件目录）为基准，绝对路径直接使用。
Debug 调用 Base 的 `mhf.quest.launch.v1` 准备本地会话，随后 Base 在 attach 安装任务 Hook。
登录服务、编码和记住密码的行为见 [Login](../../mods/login/README.md)，
任务与游戏内操作见 [Debug](../../mods/debug/README.md)。

Cargo features 决定可用的启动提供方，`[mods]` 决定实际选择。
当前保留原生文本与 CP932 任务，Unicode／Translation crate 暂未接入应用。
共享游戏字段、字体、INI 和 Mod 配置语义见 [游戏库](../../runtime/game/README.md#共享配置与资源)。

## 代码结构

- [`src/main.rs`](src/main.rs)：参数、发现、列表／导出和游戏库调用。
- [`src/builtins.rs`](src/builtins.rs)：编译能力、Factory 及内置 Base／Debug 注册表接线。
- [`mods/login`](../../mods/login/README.md)、[`mods/debug`](../../mods/debug/README.md)：启动提供方。

内置清单由 `mhf_mod_package::BuiltinCatalog` 提供；配置和路径准备位于应用的
[`src/runtime.rs`](src/runtime.rs)，固定客户端描述是 `mhf_game::runtime::PROFILE`。
游戏设置类型及 INI 映射属于 `mhf-base`，配置桥仅存储与执行注册规则；
共享界面组件来自 `shared/egui-hunter`。

## 构建和启动

游戏 DLL 为 32 位，必须构建 i686 版本。原生 Windows 安装 MSVC 构建工具和 Windows SDK 后，
在 `mhf/` 目录执行：

```sh
cargo build -p mhf-launcher --release --target i686-pc-windows-msvc
```

未提供 `-c/--config` 时读取当前工作目录的 `mhf.toml`；未提供 `-d/--game-dir` 时使用启动器目录。
显式传入的相对路径以进程的调用目录为基准。游戏目录用于定位 `mhfo-hd.dll`，无需物理 `mhf.ini`：

```text
mhf-launcher.exe
mhf-launcher.exe --config mhf.toml --game-dir D:\mhf
mhf-launcher.exe --help
mhf-launcher.exe --list-mods
mhf-launcher.exe --export-modpack selected.zip --game-dir D:\mhf
```

在 macOS/Linux 的仓库根目录进入 flake 开发环境后：

```sh
nix develop --impure
mhf-build
mhf-launcher
mhf-mods-build
mhf-mods
mhf-mods list
MHF_CONFIG=mhf.local.toml mhf-launcher
```

启动器的 `build.rs` 聚合领域 API 的头文件。验证和导出命令见
[头文件生成](../../../docs/dll-mods.md#头文件生成)。跨平台执行 Windows 目标测试需设置
`CARGO_TARGET_I686_PC_WINDOWS_MSVC_RUNNER`；`development.mhf.runner` 配置开发命令的运行器。

## Nix 开发环境

`mhf/default.nix` 定义构建、启动命令和默认禁用的启动器进程。本机设置位于忽略 Git 的 `local/default.nix`：

```nix
{ ... }: {
  development.mhf = {
    gameDirectory = "/path/to/mhf";
    runner = "wine";
  };
}
```

Direnv 自动加载本地模块；手动使用本地模块时加 `--impure`：

```sh
nix develop --impure
nix run --impure .#mhf-launcher
nix run .#mhf-build
nix run --impure .#mhf-mods
```

`mhf-build` 构建同一个 `mhf-launcher`，默认包含 Login 与 Debug。
`development.mhf.debug.enable = false;` 可移除 Debug 实现，管理器共用该编译能力设置。

Flake 提供 LLVM 和 x86 Windows SDK/CRT，设置 i686 专用编译、归档和链接环境，命令使用普通 `cargo build`。
SDK 由 Nixpkgs 的 xwin 构建步骤准备，项目接受其 Microsoft 软件许可。RustRover 继承开发环境后重新加载 Cargo 即可。
Flake 输出覆盖 macOS/Linux；原生 Windows 使用 Cargo 和 EXE，WSL 使用 Linux 输出。

Cargo 会判断构建输入是否变化并复用未变化的产物。游戏目录由 `development.mhf.gameDirectory`
提供。两个 Nix 运行命令保留调用目录，并按同一顺序选择配置：显式 `--config`、
`MHF_CONFIG`、当前工作目录的 `mhf.toml`。配置文件必须已存在；不生成运行副本，
不回退到 `.state/config` 或固定的 `local` 目录。配置中的相对 Mod 目录也按调用目录解析。
启动方式独立选择：macOS/Linux 默认使用 Wine；检测到 WSL 的 Windows 互操作
已启用时直接执行 EXE，通过 `wslpath` 转换配置和游戏目录，并通过 `WSLENV`
转发 `MHF_*` 环境变量。显式设置 `development.mhf.runner`
可指定 Wine 可执行文件，设为空字符串则直接执行 EXE；`WINEPREFIX` 默认为
`$PROJECT_STATE/wine`，公共状态目录默认是仓库的 `.state/`。原生 Windows 直接执行 EXE。
两个运行命令设置默认 WINEPREFIX；进入开发环境或编译时不会初始化 Wine。
正常启动器的默认 endpoint 是 HTTP，端口来自 Nix 选项
`development.ports.signHttp`（53001）。在 `local/default.nix` 中设置
`mhf.sign.endpoint = "tcp://127.0.0.1:53000";` 即可使用 TCP 登录；需要跟随服务端
端口配置时可引用 `config.development.ports.signTcp`。
环境变量 `MHF_SIGN__ENDPOINT` 覆盖完整地址，`MHF_SIGN__ENCODING` 覆盖 TCP 文本编码。
Erupe 的本地 Nix 配置可设置 `mhf.sign.encoding = "shift_jis";`。例如：

```sh
MHF_SIGN__ENDPOINT=tcp://127.0.0.1:53000 mhf-launcher
```

`shrimpman-dev up` 启动服务端和 etcd；启动器也可
在 process-compose 的 TUI 中手动启动。命令和环境变量覆盖详见仓库根目录 README。
