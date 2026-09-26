# Shrimpman 服务端

Shrimpman 是 Rust 服务端工作区，提供账户登录与角色管理、服务器列表以及 Land 连接的登录和心跳处理。服务间通过 etcd 发现实例，Sign 与 World 使用 SQLite 持久化数据。

## 文档导航

- [架构与模块职责](docs/architecture.md)：crate 分层、连接生命周期与功能边界。
- [配置与运行](docs/configuration.md)：配置优先级、服务端口、环境变量与数据库。
- [协议与接口](docs/protocol.md)：TCP 命令、HTTP API、文本编码与字节限制。
- [领域术语](docs/domain-language.md)：World、Land、角色位置等概念。
- [仓库开发环境](../docs/development.md)：Nix 环境、构建与开发进程。
- [仓库验证说明](../docs/validation.md)：检查命令与平台要求。

## 启动

在仓库根目录进入开发环境并启动进程组：

```sh
nix develop --impure
shrimpman-dev up
```

进程组会启动 etcd、执行数据库迁移，再启动服务。Sign 和 World 等待迁移成功，所有服务等待 etcd 健康检查通过。单独选择服务时也会包含其依赖：

```sh
shrimpman-dev up shrimpman-entrance
shrimpman-dev up shrimpman-sign
shrimpman-dev up shrimpman-world
```

构建与数据库命令：

```sh
shrimpman-build
shrimpman-migrate
shrimpman-db --help
nix run .#dev -- up
nix run .#shrimpman-db -- migration status
```

开发进程与命令由 [default.nix](default.nix) 定义，生成的服务配置来自 [config.nix](config.nix)。

## 验证

在仓库根目录执行；需具备 Rust 工具链与 Protocol Buffers 编译器：

```sh
cargo fmt --manifest-path shrimpman/Cargo.toml --all --check
cargo clippy --manifest-path shrimpman/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo test --manifest-path shrimpman/Cargo.toml --workspace --locked
```

单元测试使用内存数据库与本机临时 TCP 端口，不要求运行中的 etcd。真实服务发现和完整客户端交互需在开发进程组中另行验证。
