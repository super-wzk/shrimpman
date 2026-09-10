# Config

`mhf.config` 是独立配置 Mod，通过 `mhf.config.v1` 提供节注册、读取与写入，
并执行消费者声明的 Win32 INI 映射。它不依赖 Base、Login 或包管理 schema。
默认 Cargo 层只有公开 API；`provider` 增加 Store、服务与 Windows 原生桥接。

配置只有一份 UTF-8 `mhf.toml`。Base 注册游戏字段和八个游戏 INI 节的映射；
Login 注册并解析 `[sign]`，应用解释 `[mods]`。
添加消费者配置无需向本 crate 增加领域类型或硬编码分支。

## 注册与访问

`Registration` 包含 `defaults`、`fixed` 和可选 `ini`，默认均为空。
读取按默认值、当前文件、固定值的顺序合并。默认值应用在内存中，不因此改写用户文件；
固定值最后生效，也约束持久化结果。INI 映射通过节名、键、TOML 路径和通用值类型描述，
枚举映射及整数范围由注册者提交。

消费者在清单中依赖 `mhf.config`，通过 `Config::bind` 取得借用包装。
以下示例在内部 `Module` 使用的 `Result<_, String>` 中转换 SDK 错误：

```rust,ignore
let config = mhf_config::Config::bind(dependencies)
    .map_err(|error| error.to_string())?;
config.register("example", &mhf_config::Registration::default())
    .map_err(|error| error.to_string())?;
let current = config.read("example").map_err(|error| error.to_string())?;
config.write("example", "enabled = true")
    .map_err(|error| error.to_string())?;
```

读取结果和写入补丁都是该节的 TOML 内容，不包含外层节标题。
写入把补丁合并到最新文件，保留未修改字段；验证失败不写文件。相同节重复注册要求定义一致。
原生 INI 调用只按已注册映射处理；未注册的纯字符串表保留原有兼容读取。

`ConfigTable` 是提供方拥有的借用表，`Config` 可复制但不会延长提供方或 DLL 生命周期。
`register`、`read`、`write` 和 `last_error` 均使用公开函数表，不向消费者暴露 Store。
具体 C 声明由启动应用聚合到 [`mhf_game.h`](../../apps/launcher/include/mhf_game.h)。

## 共享配置与资源

应用通过 `Store::load` 打开配置并解析启动所需路径；游戏启动器和 Mod 管理器均以调用目录解析默认配置。Base 的设置及 INI 定义见 [Base](../base/README.md)，
登录编码和环境覆盖见 [Login](../login/README.md)。
