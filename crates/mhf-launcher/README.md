# MHF launcher

这个 Windows PE32 应用使用一个 UTF-8 `mhf.toml` 提供游戏配置和模拟 Sign 登录
数据，再启动 `mhfo.dll` 或 `mhfo-hd.dll`。`[server]` 使用 Sign 领域中的命名；
其余已建模配置同样使用小写下划线的领域命名，不需要 `ini.` 前缀：

```toml
[server]
entrance_servers = ["127.0.0.1:53310"]
last_character_id = 1

[server.credentials]
username = "user_abc"
password = "123456"

[server.session]
session_id = 1
token = "KySJuNnR2PJu00Uw"

[[server.characters]]
id = 1
name = "char_abc"

[screen]
mode = "windowed"
window_resolution = { width = 400, height = 400 }

[font]
quality = "antialiased"
weight = 0x2bc
name = "MS Gothic"

[localization]
language = "japanese"
```

已建模字段使用 TOML 原生 boolean、integer 和 string；枚举使用稳定的语义字符串，
例如 `windowed`、`high_definition` 和 `antialiased`。语言可选 `japanese`、
`english`、`korean` 和 `traditional_chinese`。`set`、`screen`、`video`、`sound`、
`localization`、`font`、`option` 和 `launch` 会解析为强类型配置；游戏动态创建的其他
section/key 仍以 string 原样保留。

加载游戏 DLL 前，启动器使用 MinHook 的 `create_hook_api` 拦截
`GetPrivateProfileIntA`、`GetPrivateProfileStringA` 和
`WritePrivateProfileStringA`。目标 `mhf.ini` 是虚拟文件名，所有读写实际落到
`mhf.toml`；其他 INI 请求仍转发给原始 Win32 API。hook 边界负责在 UTF-8 和
当前 Windows ANSI 代码页之间转换，并把领域字段映射为游戏使用的大写 INI 名称、
`0`/`1` boolean 和数字枚举。游戏写回时执行反向映射；TOML 会被重新格式化，原
注释不保证保留。

每次启动都保持 `mhf_mutex_number = 0`，并用当前进程 ID 创建独立的
`MHF_MASTER` 与 `MHF_MASTER_READY` 互斥量，因此允许多开。

## 代码结构

- `src/abi.rs`：32 位 `repr(C)` 结构、函数签名和布局断言。
- `src/model.rs`：定义完整领域 `Config`、游戏 `MhfConfig`、Sign 登录结果和启动
  profile；角色、会话与权限复用 workspace 领域类型。
- `src/launcher.rs`：负责领域模型到 ABI 的映射及 Win32 启动流程。
- `src/bin/mhf-launcher/config.rs`：在 TOML 持久化格式与强类型 `Config` 之间转换；
  原始 `toml::Table` 只封装在私有字段的 `Store` 中。
- `src/bin/mhf-launcher/ini_hook.rs`：把 Win32 Profile API 代理到 TOML。
- `src/bin/mhf-launcher/runtime.rs`：处理路径、文件和运行编排。

profile 只使用 Rust 的 `&str`；DLL 名、INI 名、互斥量前缀和宿主提示文本由
`main` 传入，`CString`/`PCSTR` 转换留在 Win32 边界。固定的 `mhDLL_Main` ABI
入口由 library 定义。领域 `Config` 使用 `bool`、枚举、`Ipv4Addr`、
`SocketAddrV4`、`CharacterId`、`SignSessionId`、`CourseRights`、`Timestamp` 和固定
长度 token；DLL 所需的原始 `u32` 只出现在 ABI 映射边界。crate 只支持 i686
Windows。

## 构建和启动

DLL 均为 32 位，因此必须构建 i686 版本：

```text
cargo build -p shrimpman-mhf-launcher --release --target i686-pc-windows-msvc
```

启动器参数依次为 TOML 路径和游戏目录。相对 TOML 路径以启动进程的当前工作
目录为基准；默认文件名为 `mhf.toml`。游戏目录用于定位 `mhfo[-hd].dll`，不再
要求存在物理 `mhf.ini`：

```text
mhf-launcher.exe mhf.toml D:\\mhf
```

在 workspace 根目录使用 Just 时，Windows 直接运行 `.exe`，其他系统自动添加
`wine` 前缀：

```text
just mhf-launch
just mhf-launch mhf.local.toml
```

非 Windows 系统使用 `cargo-xwin --xwin-arch x86` 构建；Windows 使用原生 Cargo。
Just 会为 MinHook 的 C 静态库构建自动提供基于 `lld-link /lib` 的 `llvm-lib`
兼容入口，并避免 macOS `ranlib` 破坏生成的 COFF 库。游戏目录由 `.env` 中的
`MHF_GAME_DIR` 提供。
