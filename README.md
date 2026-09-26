# Shrimpman

Monster Hunter Frontier 服务端与 HD 客户端工具。仓库包含两个独立 Rust 工作区：

| 工作区 | 用途 | 文档入口 |
| --- | --- | --- |
| `shrimpman/` | Sign 登录、Entrance 分流、World 游戏服务及持久化 | [服务端说明](shrimpman/README.md) |
| `mhf/` | Windows x86 游戏宿主、启动器、Mod 管理器、资源工具与功能组件 | [客户端说明](mhf/README.md) |

客户端固定面向 `mhfo-hd.dll`，游戏运行目标为 `i686-pc-windows-msvc`。
服务端协议、客户端原生调用及资源格式的适用范围见各模块文档。

## 快速开始

安装支持 `nix-command` 和 `flakes` 的 Nix，在仓库根目录运行：

```sh
nix develop --impure
shrimpman-dev up
```

开发环境提供 Rust、etcd、Protobuf、LLVM 和 Windows x86 SDK/CRT。
服务由 Process Compose 管理，等待 etcd 健康检查与数据库迁移后启动。
进入开发环境只加载工具与变量，服务需要显式启动。

启动 HD 客户端前，在忽略提交的 `local/default.nix` 中设置游戏目录：

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

## 开发与维护

| 需要做什么 | 文档 |
| --- | --- |
| 配置 Nix、direnv、构建命令与服务进程 | [开发环境](docs/development.md) |
| 调整端口、本地覆盖、运行路径和生成配置 | [配置说明](docs/configuration.md) |
| 格式检查、Rust 测试、Windows 构建和 ABI 验证 | [验证指南](docs/validation.md) |
| 理解服务端边界与协议 | [服务端文档](shrimpman/README.md) |
| 理解游戏宿主、组件和 Mod 生命周期 | [Mod 系统](mhf/docs/mod-system.md) |
| 编写、组合和分发 DLL Mod | [DLL Mod](mhf/docs/dll-mods.md) |
| 管理 Hook 所有权、冲突及释放 | [Hook 契约](mhf/docs/mod-hooks.md) |
| 解析和编辑游戏资源 | [资源格式](mhf/crates/shared/resource/README.md) |

只有仓库根目录是 Nix flake；两个 Rust 工作区各自维护 `Cargo.toml` 和 `Cargo.lock`。
Mod 示例还有独立工作区，构建方式见[示例说明](mhf/examples/mods/README.md)。
