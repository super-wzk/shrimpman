# 协议与接口

## TCP 协议

三个服务使用 `transport` 中的 MHF 加密帧与校验规则。业务字段采用大端序；具体布局以服务用例的 `inbound` / `outbound` 和对应字节向量测试为准。

| 服务 | 请求或控制项 | 行为 |
| --- | --- | --- |
| Sign | `SIGN:`、`DSGN:`、`DLTSKEYSIGN:` | 用户名密码登录并签发会话 |
| Sign | `DELETE:` | 删除会话所属账户的角色 |
| Entrance | `ALL` | 返回可用 World / Land 列表 |
| Entrance | `ALL+` | 返回列表；携带角色 ID 时附加按请求顺序排列的位置占位 |
| World | `0x0014` Login | 验证 Sign 会话、绑定角色、返回服务端时间 |
| World | `0x0017` Ping | 返回 ACK |
| World | `0x0011` NOP、`0x0010` END、`0x0012` ACK | 控制包、分组结束与响应关联 |

Sign 命令后接三位 ASCII 版本号，再接 NUL，例如 `SIGN:041\0`；Entrance 命令以 NUL 结束。Sign 和 Entrance 先接收 8 字节初始化段。World 没有初始化段，一个传输载荷可以包含多个包，结束标记必须位于分组末尾。

Sign 登录用户名尾部的 `+` 是 TCP 适配层的角色创建标记，验证账户时会去除该标记。HTTP 用户名按原值处理。

## HTTP JSON API

Sign 同时提供 JSON API。请求使用 `Content-Type: application/json`。

| 方法与路径 | 请求字段 | 成功响应 |
| --- | --- | --- |
| `POST /sign-in` | `username`、`password` | `200`，返回会话、角色、入口地址、公告及活动信息 |
| `POST /characters` | `session_id`、`session_token` | `201`，返回新角色 |
| `DELETE /characters/{character_id}` | JSON 中的 `session_id`、`session_token` | `204`，无响应体 |

登录响应中的 `session` 含 `session_id`、`token`、`issued_at`；后续角色请求将 `token` 作为 `session_token` 传入。令牌的在线长度为 16 字节。新建角色处于未初始化状态，同一账户存在未初始化角色时，再次创建返回 `409 pending_character_exists`。

错误响应为 `{"error":"错误代码"}`。无效请求返回 `400 invalid_request`，空用户名返回 `400 illegal_input`，凭证错误返回 `401 wrong_password`，会话无效返回 `401 invalid_session`，角色不属于当前账户或不存在时返回 `404 character_not_found`，内部错误返回 `500 internal_error`。

HTTP 登录只返回已有角色；需要创建角色时显式调用角色接口。接口字段定义分别见 [登录](../crates/sign/src/http/password_sign_in.rs)、[角色创建](../crates/sign/src/http/create_character.rs)和[角色删除](../crates/sign/src/http/delete_character.rs)。

## 文本编码与字节限制

Sign、Entrance 的在线文本使用 UTF-8。Sign 拒绝不是 UTF-8 的凭证，出站文本不进行替换或截断。配套客户端与启动器必须使用一致的编码约定。

| 字段 | UTF-8 文本有效载荷上限 |
| --- | --- |
| Sign 角色名 | 15 字节，16 字节字段内保留末尾 NUL |
| Sign 角色描述 | 31 字节，32 字节字段内保留末尾 NUL |
| Sign 登录公告 | 65,534 字节，u16 长度包含末尾 NUL |
| Sign 服务器地址或关联名称 | 254 字节，u8 长度包含末尾 NUL |
| Entrance World 名称与描述 | 两者合计 63 字节，65 字节字段内保留两个 NUL |

上限按字节计算，中文等多字节字符不能按字符数估算。HTTP JSON、配置、数据库文本和密码哈希输入使用 Unicode 字符串。`savedata` 是不透明二进制；如果解析其中的文本，必须依据具体格式处理字段，不能把整个数据块当作 UTF-8 字符串改写。
