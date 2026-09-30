# 开发环境

[返回项目入口](../README.md) · [配置](configuration.md) · [验证](validation.md)

## 工具链与平台

根 `flake.nix` 声明依赖与支持平台，`flake.lock` 固定 Nixpkgs、Rust overlay、
flake-parts、git-hooks.nix 和 process-compose-flake。
[`rust-toolchain.toml`](../rust-toolchain.toml) 定义 Rust 组件与 Windows 目标。
Login 的客户端协议代码位于本工作区的 `mhf/crates/shared/shrimpman-{common,domain,transport}/`。

| 主机 | 构建 | Windows 程序执行 |
| --- | --- | --- |
| Apple Silicon macOS | Cargo + Nix LLVM、Windows x86 SDK/CRT | 主机已安装的 Wine |
| aarch64/x86_64 Linux | Cargo + Nix LLVM、Windows x86 SDK/CRT | 可用的 Wine；x86_64 shell 包含 Wine |
| WSL | 使用对应 Linux flake 输出 | 开启互操作时直接运行 EXE，通过 `wslpath` 转换路径 |
| 原生 Windows | Cargo + Windows MSVC 工具链 | 直接运行 EXE；不使用本仓库 Nix 输出 |

Nix 设置 `clang-cl`、`llvm-lib`、`lld-link` 和 SDK 搜索路径，构建命令使用普通 `cargo build`。
交叉编译成功不代表主机能够执行 Windows 程序；测试还需要运行器，见[验证指南](validation.md)。

## 进入环境

从仓库根目录运行 `nix develop --impure`，包含存在的 `local/default.nix`。
使用纯共享配置时运行 `nix develop`。本地模块选项见[配置说明](configuration.md)。

需要自动激活时，在项目 shell 外安装 direnv，并在 `~/.zshrc` 中添加
`eval "$(direnv hook zsh)"`，然后从仓库根运行：

```sh
direnv allow
```

[`.envrc`](../.envrc) 加载固定版本 nix-direnv，监视工具链、Nix 模块及 `local/`，
使用 `--impure` 加载本地覆盖。环境初始化解析 `PROJECT_ROOT`、`PROJECT_STATE` 并创建状态目录；
应用命令选择工作目录后执行，Process Compose 继承相同环境。

## 常用命令

以下 `nix run` 命令从仓库根运行；添加 `--impure` 可包含本地模块。
包装命令先进入对应 devShell，使编译器、SDK 和库路径完成初始化。

| 仓库根命令 | shell 内命令 | 用途 |
| --- | --- | --- |
| `nix run .#dev -- up` | `mhf-dev up` | 打开客户端开发进程组 |
| `nix run .#mhf-build` | `mhf-build` | 构建 Windows 启动器 |
| `nix run .#mhf-launcher -- --config mhf/mhf.toml` | `mhf-launcher --config mhf/mhf.toml` | 构建并启动游戏 |
| `nix run .#mhf-mods-build` | `mhf-mods-build` | 构建独立 Mod 管理器 |
| `nix run .#mhf-mods -- --config mhf/mhf.toml` | `mhf-mods --config mhf/mhf.toml` | 构建并打开管理器 |
| `nix run .#mhf-ai-decompile-build` | `mhf-ai-decompile-build` | 构建 AI 导出工具 |
| `nix run .#mhf-ai-decompile -- 1 31` | `mhf-ai-decompile 1 31` | 构建并运行 AI 导出工具 |

运行 MHF 工具的参数和用途分别见[启动器](../mhf/crates/apps/launcher/README.md)、
[管理器](../mhf/crates/apps/mod-manager/README.md)和 [AI 导出](../mhf/crates/apps/ai-decompile/README.md)。
`apps` 是可运行入口，`packages` 是可构建产物，`checks` 是 `nix flake check` 执行的检查。
`update-configs` 仅生成公开 TOML，不进入 devShell。

## 客户端进程

```sh
mhf-dev up
mhf-dev up -t=false
mhf-dev process list
```

`mhf-launcher` 进程默认禁用，可在 Process Compose TUI 中显式启动。
也可直接运行 `mhf-launcher --config mhf/mhf.toml`。
登录服务在独立的 `shrimpman-server` 仓库中启动，客户端通过完整 Sign URI 连接。

## 模块职责

| 文件 | 职责 |
| --- | --- |
| [`development/flake.nix`](../development/flake.nix) | 装配开发模块，导出 apps、packages、checks |
| [`development/shell.nix`](../development/shell.nix) | 公共选项、devShell、命令与路径辅助函数 |
| [`development/git-hooks.nix`](../development/git-hooks.nix) | 提交钩子、格式与语法检查 |
| [`mhf/default.nix`](../mhf/default.nix) | Windows 构建、Wine/WSL 运行 |
| [`mhf/config.nix`](../mhf/config.nix) | MHF 默认值与 TOML 生成 |
| `local/default.nix` | 不提交的机器配置；存在且启用 impure 求值时加载 |

编译依赖库放入 `development.buildInputs`，可执行工具放入 `development.packages`。
公共默认值更新时同时刷新生成配置，提交前执行[验证指南](validation.md)中的适用检查。
