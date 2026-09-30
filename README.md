# MHF 客户端工具

Monster Hunter Frontier HD 客户端宿主、启动器、Mod 管理器、资源工具与功能组件。
Rust 工作区位于 [`mhf/`](mhf/README.md)，客户端固定面向 `mhfo-hd.dll`，
游戏运行目标为 `i686-pc-windows-msvc`。

## 快速开始

Login 使用本仓库 `mhf/crates/shared/` 下的客户端协议代码副本：
`shrimpman-common`、`shrimpman-domain` 和 `shrimpman-transport`，
分别提供二进制基础类型、领域模型与 MHF 帧传输。

安装支持 `nix-command` 和 `flakes` 的 Nix，在仓库根目录运行：

```sh
nix develop --impure
```

开发环境提供 Rust、LLVM 和 Windows x86 SDK/CRT。在忽略提交的 `local/default.nix`
中设置游戏目录：

```nix
{ ... }: {
  development.mhf.gameDirectory = "/path/to/mhf";
}
```

重新进入开发环境，在仓库根目录运行：

```sh
mhf-launcher --config mhf/mhf.toml
```

启动器直接读写指定配置；需要独立配置时，先将 `mhf/mhf.toml` 复制到自己的运行目录。
默认启动模式为 Login，Debug 和 Workbench 通过配置启用，见[启动选择与配置](mhf/crates/apps/launcher/README.md#启动选择与配置)。
macOS/Linux 执行 Windows 程序需要可用的 Wine；WSL 可使用 Windows 互操作。

Login 默认连接 `http://127.0.0.1:53001`。Sign 等服务由独立的 `shrimpman-server`
仓库提供；客户端通过 `mhf.sign.endpoint` 或 `MHF_SIGN__ENDPOINT` 设置完整登录地址。

## 开发与维护

| 需要做什么 | 文档 |
| --- | --- |
| 配置 Nix、direnv、构建命令与客户端进程 | [开发环境](docs/development.md) |
| 调整本地覆盖、运行路径和生成配置 | [配置说明](docs/configuration.md) |
| 格式检查、Rust 测试、Windows 构建和 ABI 验证 | [验证指南](docs/validation.md) |
| 理解游戏宿主、组件和 Mod 生命周期 | [Mod 系统](mhf/docs/mod-system.md) |
| 编写、组合和分发 DLL Mod | [DLL Mod](mhf/docs/dll-mods.md) |
| 管理 Hook 所有权、冲突及释放 | [Hook 契约](mhf/docs/mod-hooks.md) |
| 解析和编辑游戏资源 | [资源格式](mhf/crates/shared/resource/README.md) |

仓库根目录是 Nix flake，`mhf/` 维护客户端的 `Cargo.toml` 和 `Cargo.lock`。
Mod 示例还有独立工作区，构建方式见[示例说明](mhf/examples/mods/README.md)。
