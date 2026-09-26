# 领域术语

领域模型和应用用例使用下列术语，协议字段名或参考实现名称只在适配层保留。

## 服务器选择层级

```text
Sign Service
└── Entrance Service
    └── World
        └── Land
            └── Land Server
```

此层级表达选择路径与职责。一个 World 进程可以同时监听多个 Land；Land 是客户端可见的目的地，Land Server 是为其提供连接的服务端。

## 领域概念

| 术语 | 含义 |
| --- | --- |
| World | Entrance 返回的 Land 分组 |
| World type | World 用途分类：Free、Dundorma Town、Beginner、Public Tavern、Returning Hunter、Mezeporta Festa |
| World season | 繁殖期、温暖期或寒冷期，对应 Breeding、Warm、Cold |
| World content | World 提供的任务范围或小游戏内容 |
| Land | World 内可连接的目的地，包含端口、容量和在线人数信息 |
| Character presence | 指定角色已知的活动 World / Land；未知位置表示没有可用的位置记录 |
| Sign session | Sign 签发的、有期限的账户凭证，用于后续角色操作与 World 登录 |
| World session | 角色绑定到 Land 连接后的进程内活动会话 |

## 服务职责

| 术语 | 职责 |
| --- | --- |
| Sign Service | 认证账户、签发 Sign session、管理角色并提供 Entrance 地址 |
| Entrance Service | 提供 World / Land 列表与角色位置响应 |
| World | 管理该 World 下的 Land 监听与活动角色会话 |
| Land Server | 接受对应 Land 的连接 |
| Discovery | 按服务名发布、订阅和选择服务实例的基础设施 |

已知具体服务时应直接使用其名称，避免用不带限定词的 `Server` 混指业务服务、World、Land 和 TCP 监听器。角色位置、在线人数等数据的实际提供范围见[架构文档](architecture.md#功能边界)。
