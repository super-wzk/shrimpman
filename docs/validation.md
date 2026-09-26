# 验证指南

[返回项目入口](../README.md) · [开发环境](development.md) · [配置](configuration.md)

以下命令从仓库根运行。两个 Rust 工作区分别检查；Windows 原生模块需要目标工具链，
依赖游戏文件的检查还需要相应资源。执行结果应区分编译、自动化测试和真实游戏交互。

## 格式与工程配置

```sh
cargo fmt --manifest-path shrimpman/Cargo.toml --all --check
cargo fmt --manifest-path mhf/Cargo.toml --all --check
```

进入 Nix shell 会通过 git-hooks.nix 安装 `prek` 提交钩子，检查 Nix 格式、受影响工作区的
Rust 格式、TOML 语法及合并冲突。生成的 `.pre-commit-config.yaml` 由 Git 忽略。

```sh
nix develop --command prek run --all-files
nix flake check
```

修复格式时去掉 `cargo fmt` 的 `--check`；Nix 文件使用开发环境的 `nixfmt`。
`nix flake check` 同时检查生成 TOML 一致性，不代替 Rust 测试或 Windows 游戏验证。
Git flake 只包含已跟踪文件，新增 crate 或拆分出的模块必须纳入 Git 跟踪后才会进入该检查。

## 服务端

在具备 Protobuf 的环境中运行：

```sh
cargo test --manifest-path shrimpman/Cargo.toml --workspace --locked
cargo clippy --manifest-path shrimpman/Cargo.toml --workspace --all-targets --locked -- -D warnings
```

etcd 与实际服务启动由[开发进程](development.md#服务进程)提供。
纯编解码和模型测试通过后，还应按所改模块验证登录、分流、租约和持久化交互。

## 客户端可移植模块

依赖解析、包归档及资源解析等模块可在宿主执行，不需要启动游戏。
从仓库根运行以下命令时，不会加载 `mhf/.cargo/config.toml` 的默认 Windows 目标：

```sh
cargo test --manifest-path mhf/Cargo.toml --locked -p mhf-mod-package -p mhf-mod-host -p mhf-mod-sdk -p mhf-launcher-catalog
cargo test --manifest-path mhf/Cargo.toml --locked -p mhf-resource
```

如果从 `mhf/` 内运行，须用 `--target` 显式指定宿主 triple 来覆盖默认目标。
某些原生实现受 `cfg(windows)` 限制；宿主测试不能覆盖这些路径。
资源样本、AI 与原生模型检查的输入要求见[资源文档](../mhf/crates/shared/resource/README.md)、
[Monster](../mhf/crates/mods/monster/README.md)和 [Geometry](../mhf/crates/mods/geometry/README.md)。

## Windows x86 与 ABI

在 Nix shell 或已配置 Windows MSVC 工具链的环境中运行：

```sh
cargo check --manifest-path mhf/Cargo.toml --workspace --all-targets --target i686-pc-windows-msvc --locked
cargo build --manifest-path mhf/Cargo.toml -p mhf-launcher --bin mhf-launcher --release --target i686-pc-windows-msvc --locked
```

跨平台运行 Windows 测试需要通过 `CARGO_TARGET_I686_PC_WINDOWS_MSVC_RUNNER` 指定可用的 Wine 命令。
仅安装编译目标或设置 `development.mhf.runner` 不会为 Cargo 测试自动配置运行器。

公开 C 头文件由 Rust 定义生成，仓库快照通过集成测试检查：

```sh
cargo test --manifest-path mhf/Cargo.toml -p mhf-launcher --target i686-pc-windows-msvc --test headers --locked
```

Mod 示例拥有独立工作区，需要分别检查 Counter 与 Hook 的头文件。
更新快照使用 `MHF_UPDATE_HEADERS=1`，导出使用 `MHF_HEADERS_EXPORT_DIR`；
完整命令和所有输出见[头文件生成](../mhf/docs/dll-mods.md#头文件生成)。

## 游戏运行验证

[Mod 示例](../mhf/examples/mods/README.md)检查跨 DLL 调用与 Hook 恢复。
[HD 会话测试](../mhf/crates/runtime/game/src/game/tests.rs)加载真实 DLL 并执行 DllMain，
不运行游戏主入口。实际游戏中的 UI、输入、任务、迟到回调及退出仍需单独验证。

涉及生命周期或原生缓冲的修改，应验证依赖顺序、安装失败清理、停止与排空、DLL 释放及重复启动。
涉及 ABI 或二进制格式的修改，应同时检查布局、边界条件和已有资源样本，保持不确定字段的原始数据。
