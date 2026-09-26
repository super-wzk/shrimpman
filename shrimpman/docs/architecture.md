# 架构与模块职责

## 工作区结构

| crate | 职责 |
| --- | --- |
| `domain` | 账户、角色、会话、World/Land、活动及时间范围等领域模型 |
| `common` | 固定宽度字符串、长度前缀集合、布尔值和时间戳等二进制基础类型 |
| `transport` | MHF 帧边界、校验、加解密与异步连接 |
| `protocol` | 包流、命令解码、路由表、处理器调度与出站队列 |
| `runtime` | 服务进程的配置、日志、退出信号，以及 Sign/Entrance 共用的 TCP 生命周期 |
| `lease-kv` | etcd 连接、租约续期、键值发布、重连与前缀订阅 |
| `discovery` | 将租约键值映射为服务实例快照，并提供 Ready 实例选择 |
| `persistence` | Toasty 模型、仓储、SQLite 连接配置与迁移命令 |
| `sign` | TCP/HTTP 登录、Sign 会话签发、角色创建与删除 |
| `entrance` | 查询可用 World/Land，以及角色位置响应 |
| `world` | 每个 Land 的 TCP 监听、角色会话、登录与心跳 |

`domain` 不依赖服务、数据库或网络。`transport` 不解析业务命令；`protocol` 不认识具体服务。服务 crate 的 `application` 组织用例，`router` 注册协议命令，HTTP/TCP 适配层将协议数据转换为用例请求。

## 请求路径

```text
客户端
├─ Sign TCP / HTTP → 登录与角色管理 → SQLite
│  └─ Discovery → 选择 Entrance 端点
├─ Entrance TCP → Discovery 快照 → World / Land 列表
└─ Land TCP → World 会话 → 验证 Sign 会话与角色归属 → SQLite
```

Sign 和 Entrance TCP 连接先读取 8 字节初始化数据，然后处理一组请求并在响应完成后关闭连接。World 的 Land 连接没有该初始化段，使用持久连接；每个包组以 `MSG_SYS_END` 结束。

World 将读包、处理器执行和写包分开驱动。处理器可以向客户端发起请求并等待 ACK，因此读包不能等待当前处理器结束。请求句柄同时包含槽位与代数，用于识别槽位复用后的迟到响应。

角色登录首先验证 Sign 会话有效期、令牌和角色归属，再绑定到连接。一个 World 进程只允许一个角色拥有一个有效连接；断开连接时，清理守卫仅移除指向该连接的索引。

## 服务发现与退出

服务发布 `Ready` 实例；收到退出信号后发布 `Draining`，停止接入新连接并等待活动请求完成。超过 `shutdown_timeout` 后取消活动连接，随后撤销实例注册。

`lease-kv` 保存进程期望发布的键值，并在重连后恢复。前缀订阅先读取快照，再从下一修订号开始监听，覆盖快照与监听建立之间的变更。订阅不可用时，Discovery 清空快照，避免继续提供不可确认的端点。

## 功能边界

- World 实现登录、Ping、NOP、包组结束和 ACK 处理；完整游戏逻辑、聊天与存档读写不在当前服务接口中。
- World 配置发布的 Land `current_players` 为 `0`，尚无动态在线人数发布。
- Entrance 的 `ALL+` 按请求角色顺序返回未知位置；World 的内存会话索引不构成跨进程位置快照。
- Sign 的 HTTP 登录不会自动创建角色；TCP 登录按协议规则保证至少一个角色，并处理用户名末尾的 `+` 创建请求。
- `savedata` 作为不透明二进制保存，服务不对整个数据块进行文本解码或转码。

扩展协议时，应在对应服务的 `application/use_cases` 定义请求与处理器，通过 `router` 注册，在线格式放在适配层；共享领域含义放入 `domain`。
