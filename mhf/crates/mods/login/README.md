# Login

`mhf-login` 提供运行 Mod `mhf.login`。它只依赖运行时的 `mhf.config`，负责 Sign 登录、角色选择与系统凭据存储，
在 prepare 发布 `mhf.launch.fallback.v1`。没有普通启动提供方时，宿主才在游戏 DLL 加载前调用它；
用户关闭登录窗口时返回取消，正常结束启动。

`LoginModule::new()`／`Default` 创建 Mod。prepare 借用 `mhf.config.v1` 并注册 `sign` 节，
fallback 真被调用时才读取配置、叠加 `MHF_SIGN__` 环境值并校验端点；Debug 接管时不校验未使用的 Sign 服务。
界面完成后填充 `LaunchParams32` 和 `GlobalData32`；
内存与句柄由游戏宿主持有，Login 不保留借用指针。它只支持 i686 Windows。

默认构建自动选择 Login。启用 `mhf.debug` 等普通启动提供方后，会自动覆盖这条默认登录流程。

## Sign 服务与界面

`mhf.login` 使用 egui/eframe 提供登录和角色选择界面，通过 Sign HTTP
或 TCP 获取真实会话和角色数据。
`mhf.toml` 只使用 `[sign] endpoint` 配置服务地址，由 URI 协议选择 HTTP、HTTPS
或 TCP，例如 `http://127.0.0.1:53001`、`https://sign.example.com` 或
`tcp://127.0.0.1:53000`。TCP 必须指定端口，支持主机名、IPv4 和 `[IPv6]:port`；
HTTP/HTTPS 可以包含 API 基础路径。启动时校验协议和地址，登录界面不接受临时覆盖。

`[sign] encoding` 默认 `utf8`（Shrimpman）；连接 Erupe 时使用 `shift_jis`。
它用于编码输入框中的凭据，以及解码界面显示的角色名。TCP 返回的角色名、公告和
会话令牌保留原始字节并传给 DLL，不做 UTF-8、ASCII 或字符串结束符校验。
名字显示采用宽容解码，不改写原始字节；凭据仍须能用指定编码准确表示。
HTTP JSON 始终使用 UTF-8，响应中的文本在 JSON 解析后保存为字节数组。

```toml
[sign]
endpoint = "tcp://127.0.0.1:53312"
encoding = "shift_jis"
```

过滤表按 u16 长度前缀读取，CAPLINK 按标志读取可选字段，解析完整响应后才更新会话。

Sign 请求在后台线程执行，连接超时为 5 秒，从 DNS 解析到完整读取响应的总超时为
10 秒，覆盖登录、创建和删除角色。超时后界面恢复操作并保留当前输入。操作错误使用
底部浮层消息，显示 6 秒，只保留最新一条，不抢焦点、不拦截点击，也不改变表单布局。
浮层最多显示 240 个字符；完整错误输出到 stderr。

HTTP 模式通过 `POST /sign-in` 登录、`POST /characters` 创建待初始化角色，
通过 `DELETE /characters/{id}` 删除角色。TCP 模式使用 8 字节初始化数据和共用的
MHF 加密分帧，通过 `DSGN:100` 登录、`DELETE:100` 删除角色（Erupe 9.2 要求 `100` 后缀）；首次登录由服务端
自动补充待初始化角色。“New character”通过用户名追加 `+` 重新登录，成功后更新
整份角色列表和新签发的会话，再启动待初始化角色。`+` 是 TCP 协议保留后缀，
该模式拒绝以 `+` 结尾的账号名以及包含 NUL 的凭据。
TCP 删除失败时当前服务端不发送错误响应，启动器会在总超时后恢复操作。

选择角色并点击“Launch game”后，
Login 关闭 UI，将当前会话、角色、公告和 Entrance 地址写入宿主借用的启动缓冲区；
编码沿用本次认证使用的设置。宿主随后加载游戏 DLL 并继续各 Mod 的生命周期。
用户名、密码、会话和角色不写入 `mhf.toml`。勾选 “Remember password” 后，只有登录
成功的用户名和密码会保存到系统凭据库；原生 Windows 使用 Credential Manager，
WineCX 使用其凭据桥接写入 macOS Keychain。取消勾选并成功登录会删除此前保存的凭据。

凭据按 Sign 端点隔离，目标名称为 `Shrimpman MHF — <HTTP URL 或 tcp://host:port>`。
会话和角色只保留在当前进程中，UTF-8 `mhf.toml` 只保存服务与游戏设置。

## 实现位置

- [`src/sign/`](src/sign/mod.rs)：HTTP/TCP 客户端、响应解析和错误处理。
- [`src/ui/`](src/ui/mod.rs)：状态、视图和 eframe 适配，复用 `egui-hunter`。
- [`src/credentials.rs`](src/credentials.rs)：Windows／Wine 凭据适配。
- [`src/config.rs`](src/config.rs)：Sign schema、环境覆盖和 Shift-JIS 编解码。
- [`src/startup.rs`](src/startup.rs)：启动回调及 Sign 数据到固定 ABI 的映射。

构建和运行命令见 [启动器](../../apps/launcher/README.md)。
