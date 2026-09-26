# 服务配置与运行

## 配置来源

Sign、Entrance、World 使用同一 TOML 文件中的 `sign`、`entrance`、`world` 节。数据库迁移读取 `migration` 节。进程从 `PROJECT_CONFIG` 读取文件路径；未设置时读取当前目录的 `config.toml`。配置文件必须存在，字段默认值不能代替缺失的文件。

服务配置优先级为：Rust 字段默认值、TOML、`SHRIMPMAN_` 环境变量。环境变量用双下划线表示嵌套层级，布尔值与数值按类型解析；只有当前服务的 `lease_kv.endpoints` 使用逗号拆分列表。`RUST_LOG` 独立覆盖 `logging.filter`。

```sh
export SHRIMPMAN_SIGN__AUTO_SIGN_UP=false
export SHRIMPMAN_SIGN__SERVER__LISTEN_ADDR=127.0.0.1:53000
export SHRIMPMAN_SIGN__LEASE_KV__ENDPOINTS=http://127.0.0.1:2379,http://127.0.0.1:2381
export SHRIMPMAN_WORLD__SHUTDOWN_TIMEOUT=10s
export RUST_LOG=warn,shrimpman_sign=debug,shrimpman_runtime=info
```

Nix 开发命令自动将 `PROJECT_CONFIG` 指向生成的配置。修改 Nix 配置与本地覆盖的方法见[仓库配置说明](../../docs/configuration.md)；服务配置生成入口见 [config.nix](../config.nix)。

## 端口

开发环境显式指定端口，与仅使用 Rust 字段默认值时不同：

| 用途 | Nix 开发配置 | Rust 字段默认值 |
| --- | --- | --- |
| Sign TCP | `53000` | `53312` |
| Sign HTTP | `53001` | `53313` |
| Entrance TCP | `53002` | `53310` |
| World Land 1 / 2 | `54001` / `54002` | 必须显式配置 |
| etcd 客户端 | `2379` | `http://127.0.0.1:2379` |

`listen_addr` 决定本机监听地址；`advertise_addr` 是供其他服务或客户端使用的地址。World 用 `address` 加各 Land 的 `port` 组成对客户端公开的连接地址。跨机器连接时，应将公开地址设为客户端可达的地址。

## 通用字段

| 字段 | 含义与默认值 |
| --- | --- |
| `lease_kv.endpoints` | etcd URL 列表，默认单个 `http://127.0.0.1:2379` |
| `lease_kv.lease_ttl` | 租约有效期，默认 `15s`，必须为正整数秒 |
| `lease_kv.reconnect_delay` | 重连间隔，默认 `1s`，必须大于零 |
| `shutdown_timeout` | 等待活动连接结束的最长时间，默认 `5s` |
| `logging.filter` | tracing 过滤表达式；`shrimpman_runtime` 控制共用运行逻辑日志 |

Sign 另有 `auto_sign_up`、`session.ttl`、`database.url`、`http.listen_addr`、`server.listen_addr` 和 `server.advertise_addr`。`auto_sign_up` 的 Rust 默认值为 `false`，Nix 开发配置设为 `true`；会话默认有效期为 `5m`。

World 必须提供 `key`、`address`、`name`、`description`、`world_type`、`season`、`content`、`client_compatibility` 以及非空 `lands`。每个 Land 包含 `key`、`listen_addr`、`port`、`max_players`；同一 World 内的 Land key 和公开端口必须唯一，公开端口不能为零。枚举值及示例以 [config.nix](../config.nix) 和[领域模型](../crates/domain/src/world/model.rs)为准。

## 数据库与迁移

Sign 的 `database.url`、World 的 `database.url` 和 `migration.database_url` 应指向同一 SQLite 数据库。Nix 根据仓库状态目录生成这些路径；字段默认连接为 `sqlite://shrimpman.sqlite3`，相对路径以服务工作目录为基准。

迁移配置位于 [toasty.toml](../toasty.toml)，迁移文件位于 [toasty](../toasty)。服务启动不会自行执行迁移；`shrimpman-dev` 通过进程依赖先执行 `shrimpman-migrate`。

```sh
shrimpman-db migration status
shrimpman-migrate
```

手动使用 Cargo 运行迁移命令时，从 `shrimpman/` 工作区目录执行，以便 Toasty 找到配置和迁移目录：

```sh
cd shrimpman
cargo run --locked -p shrimpman-persistence --bin toasty -- migration status
```
