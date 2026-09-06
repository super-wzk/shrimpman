# MHF launcher

这个 Windows PE32 应用使用 egui/eframe 提供登录和角色选择界面，通过 Sign HTTP
API 获取真实会话和角色数据，再启动 `mhfo.dll` 或 `mhfo-hd.dll`。Sign API 地址由
`mhf.toml` 的 `[sign.http] base_url` 配置，登录界面不接受临时覆盖。

Sign 请求在后台线程执行，连接超时为 5 秒，从 DNS 解析到完整读取响应的总超时为
10 秒，覆盖登录、创建和删除角色。超时后界面恢复操作，保留当前输入；简短提示显示在
界面中，完整错误输出到 stderr。错误提示自动换行，长内容和小窗口支持滚动查看。

登录成功后，角色列表来自 `POST /sign-in` 的响应；“New character”调用
`POST /characters` 创建待初始化角色并自动选中。选择角色并点击“Launch game”后，
启动器先关闭 UI，再安装 INI hook 并把当前会话、角色和 Entrance 地址映射到游戏 ABI。
用户名、密码、会话和角色不写入 `mhf.toml`。勾选 “Remember password” 后，只有登录
成功的用户名和密码会保存到系统凭据库；原生 Windows 使用 Credential Manager，
WineCX 使用其凭据桥接写入 macOS Keychain。取消勾选并成功登录会删除此前保存的凭据。

UTF-8 `mhf.toml` 保存 Sign API 地址和游戏设置，但不保存任何登录数据。凭据按 Sign
API 地址隔离，目标名称为 `Shrimpman MHF — <Sign API 地址>`，可由遵守相同凭据
约定的其他 Shrimpman MHF 客户端复用；会话和角色始终只保留在当前进程中。已建模配置
使用小写下划线的领域命名，不需要 `ini.` 前缀：

```toml
[sign.http]
base_url = "http://127.0.0.1:53001"

[translation]
locale = "zh-CN"
missing = "original"

[screen]
mode = "windowed"
window_resolution = { width = 400, height = 400 }

[font]
quality = "antialiased"
weight = 400
name = "JetBrains Maple Mono NF NL HT"

[localization]
language = "japanese"
```

已建模字段使用 TOML 原生 boolean、integer 和 string；枚举使用稳定的语义字符串，
例如 `windowed`、`high_definition` 和 `antialiased`。语言可选 `japanese`、
`english`、`korean` 和 `traditional_chinese`。`set`、`screen`、`video`、`sound`、
`localization`、`font`、`option` 和 `launch` 会解析为强类型配置；游戏动态创建的其他
section/key 仍以 string 原样保留。`[sign]` 与 `[translation]` 是启动器配置命名空间，
不会暴露给游戏的 Win32 Profile API。`[translation] locale` 选择按同名 JSONL 文件
编译到 EXE 内嵌字典的额外翻译；locale ID 就是文件名，不限定语言代码格式。每个 locale
地位相同，空 JSONL 表示没有覆盖。只有显式配置 `[translation]` 才会安装翻译 hook；
省略该 section 时游戏完全使用原始文本。资源结构和合法 key 由 resource layout 定义，
每个 locale 只包含自己的实际覆盖。
`missing` 默认为 `original`，也可设为 `key` 或 `empty`。layout、key 和翻译文件格式见
[`translations/README.md`](translations/README.md)。

`[localization] language` 与翻译覆盖相互独立：前者是游戏原生语言资源选择器并写入
MHF ABI，后者只选择启动器内嵌字典。使用 `missing = "original"` 时，未命中的 key
显示所选游戏语言的原始文本。

EXE 内嵌一份 JetBrainsMapleMono-NF-XX-NL-HT Regular，启动器 UI 直接使用其静态字节；当
`[font] name` 选择该字体时，同一份字节会在进入游戏前注册为仅当前进程可见的 GDI
字体，不安装到 Windows 或 Wine 字体目录。选择其他字体时使用系统中已有的对应字体。

加载游戏 DLL 前，启动器通过 `mhf-hooks` 解析并拦截
`GetPrivateProfileIntA`、`GetPrivateProfileStringA` 和
`WritePrivateProfileStringA`。目标 `mhf.ini` 是虚拟文件名，所有读写实际落到
`mhf.toml`；其他 INI 请求仍转发给原始 Win32 API。hook 边界负责在 UTF-8 和
当前 Windows ANSI 代码页之间转换，并把领域字段映射为游戏使用的大写 INI 名称、
`0`/`1` boolean 和数字枚举。游戏写回时执行反向映射；TOML 会被重新格式化，原
注释不保证保留。

INI、汉化/GDI、D3D9 和 DirectInput 分别持有自己的 hook 组，共用 `mhf-hooks` 的 MinHook 生命周期
管理。创建失败会回滚本组已创建的目标；回调状态就绪后才逐个启用，不使用进程级的全局
启停操作。退出时先停用目标并等待 Rust 回调及原函数调用结束，再移除 trampoline；
清理错误会返回给启动流程。汉化裸汇编仍使用原有 ABI，要求游戏调用线程在
`mhDLL_Main` 返回时已结束；游戏 DLL 和翻译缓存保留到汉化 hook 清理完成。

每次启动都保持 `mhf_mutex_number = 0`，并用当前进程 ID 创建独立的
`MHF_MASTER` 与 `MHF_MASTER_READY` 互斥量，因此允许多开。

## 代码结构

- `src/abi.rs`：32 位 `repr(C)` 结构、函数签名和布局断言。
- `src/model.rs`：定义游戏 `MhfConfig`、启动时的 Sign 登录结果和启动 profile；角色、
  会话与权限复用 workspace 领域类型。
- `src/launcher.rs`：负责领域模型到 ABI 的映射及 Win32 启动流程。
- `src/overlay/input.rs`：过滤 MHF 通过 DirectInput 读取的鼠标、键盘状态，复用
  Overlay 调用处的输入策略；`input/polling.rs` 保留每个按键从按下到松开的接收方。
- `src/localization/`：在 DAT/INF/PAC 完成原生指针重定位、首次消费或复制前，按 layout
  遍历绝对指针并把字符串槽指向独立翻译缓存，使后续别名自然继承翻译；同时捕获实际
  stage 文件号，并在对应 TLK image 中修改明确登记的记录。启动时按 Unicode East Asian
  Width 的 CJK 宽度为当前 locale 建立固定的半宽/全宽虚拟字形映射，分别使用游戏原有的
  8/16 像素槽并交给 GDI 宽字符接口；运行时不解析 JSONL。
- `src/bin/mhf-launcher/http/`：Sign HTTP 客户端及按 API 命名空间组织的请求、响应
  模型。
- `src/bin/mhf-launcher/ui/`：按 Elm 结构组织状态更新、界面渲染和 eframe 适配。
- `src/bin/mhf-launcher/config.rs`：解析 `[sign.http]`、`[translation]` 和强类型
  `MhfConfig`，并将后者映射到 TOML 持久化格式；原始 `toml::Table` 只封装在私有
  `Store` 中。
- `src/bin/mhf-launcher/ini_hook.rs`：把 Win32 Profile API 代理到 TOML。
- `src/bin/mhf-launcher/runtime.rs`：准备游戏目录与配置，并在 UI 退出后执行游戏启动。
- `translations/`：`resources.json` 定义带稳定 `id` 的资源表及客户端运行时绑定；
  每个 UTF-8 JSONL 对应一个 locale，也可以为空。`build.rs` 根据 layout 生成资源 hook、
  校验翻译键，并把各 locale 的稀疏 UTF-8 覆盖编译成直接嵌入 EXE 的二进制字典；生成的
  类型化 locale 注册表负责运行时查询。

profile 只使用 Rust 的 `&str`；DLL 名、INI 名、互斥量前缀和宿主提示文本由
`main` 传入，`CString`/`PCSTR` 转换留在 Win32 边界。固定的 `mhDLL_Main` ABI
入口由 library 定义。启动时的 `Config` 使用 `bool`、枚举、`Ipv4Addr`、
`SocketAddrV4`、`CharacterId`、`SignSessionId`、`CourseRights`、`Timestamp` 和固定
长度 token；DLL 所需的原始 `u32` 只出现在 ABI 映射边界。crate 只支持 i686
Windows。

## 构建和启动

DLL 均为 32 位，因此必须构建 i686 版本。原生 Windows 安装 MSVC 构建工具和
Windows SDK 后使用 Cargo，直接运行生成的 EXE，不需要 cargo-xwin 或 Wine：

```text
cargo build -p shrimpman-mhf-launcher --release --target i686-pc-windows-msvc
```

启动器使用 clap 解析独立选项。未提供 `-c/--config` 时读取启动器同目录的
`mhf.toml`；未提供 `-d/--game-dir` 时使用启动器目录。显式传入的相对路径仍以
启动进程的当前工作目录为基准。游戏目录用于定位 `mhfo[-hd].dll`，不需要物理
`mhf.ini`：

```text
mhf-launcher.exe
mhf-launcher.exe --config mhf.toml --game-dir D:\\mhf
mhf-launcher.exe -d D:\\mhf
mhf-launcher.exe --help
```

在 macOS/Linux 的仓库根目录进入 flake 开发环境后，用开发命令构建和启动：

```sh
nix develop --impure
mhf-build
mhf-launcher
MHF_CONFIG=mhf.local.toml mhf-launcher
```

配置好 direnv 后，`direnv allow` 会通过 nix-direnv 自动加载环境。本工作区的
`mhf/default.nix` 定义构建、启动命令和默认禁用的启动器进程。机器上的设置放在
忽略 Git 的 `local/default.nix`，作为完整 Nix 模块加载：

```nix
{ ... }: {
  development.mhf = {
    gameDirectory = "/path/to/mhf";
    runner = "wine";
  };
}
```

Direnv 自动加载可选本地模块。手动使用本地模块时，从仓库根目录加 `--impure`：

```sh
nix develop --impure
nix run --impure .#mhf-launcher
nix run .#mhf-build
```

`mhf-build` 在全部受支持的 Nix 主机上使用 `cargo-xwin --xwin-arch x86`，
由 flake 提供 LLVM 工具。当前 flake 的输出仅覆盖
macOS/Linux；原生 Windows 使用上面的 Cargo 和 EXE 命令，WSL 使用 Linux 输出。
Cargo 会判断构建输入是否变化并复用未变化的产物。游戏目录由 `development.mhf.gameDirectory`
提供；Nix 默认使用 `$PROJECT_STATE/config/` 下生成配置的可写副本；脱离 Nix 时使用 `mhf/mhf.toml`，`MHF_CONFIG` 中的相对路径以 `mhf/` 为基准。
启动方式独立选择：macOS/Linux 默认使用 Wine；检测到 WSL 的 Windows 互操作
已启用时直接执行 EXE，通过 `wslpath` 转换配置和游戏目录，并通过 `WSLENV`
转发 `MHF_*` 环境变量。显式设置 `development.mhf.runner`
可指定 Wine 可执行文件，设为空字符串则直接执行 EXE；`WINEPREFIX` 默认为
`$PROJECT_STATE/wine`，公共状态目录默认是仓库的 `.state/`。原生 Windows 直接执行 EXE。
MHF 配置副本和 Wine 默认环境仅在启动器运行时准备，进入开发环境或编译时不会初始化。
Sign HTTP 地址默认使用开发环境的 Nix 选项 `development.ports.signHttp`（53001），可通过
`MHF_SIGN__HTTP__BASE_URL` 覆盖。`shrimpman-dev up` 启动服务端和 etcd；启动器也可
在 process-compose 的 TUI 中手动启动。命令和环境变量覆盖详见仓库根目录 README。

### 游戏内组件验证页

游戏启动后按 **F8** 显示或隐藏 `egui-hunter` 验证页。页面通过已有 D3D9 Overlay
渲染，复用启动器内嵌的中文字体；包含一万条虚拟列表记录、文本输入、确认框、
锚定菜单和排队通知。普通通知以轻量提示自动消失，不抢焦点、不拦截鼠标；
需要作出选择时才打开确认框。验证页最外层使用原生 Modal 隔离 egui 背景交互，
内部面板只负责标题和布局；窄窗口会切换为单列布局。

键盘打开后直接聚焦「滚动与选择」列表。Tab / Shift+Tab 沿用 egui 原生顺序，
遍历列表、表单、按钮和页面关闭入口；整个列表只占一个焦点，Tab 一次离开列表。
列表内用方向键、PageUp / PageDown、Home / End 导航，Enter 直接选择。
鼠标单次点击内部控件即可操作，标题和面板背景只负责展示。
所有业务上可用的区域和控件保持可操作，不需要先确认进入框。

页面另外声明可选的手柄 `FocusEngagement`：方向先选择整个区域，A 进入上次可用控件，
这次按键不会同时执行控件；进入后操作内部内容，B 退出区域。RB / LB 在区域层切区，
进入后切内部控件。该规则只处理有来源标记的手柄动作，不改变物理键盘行为。
当前 Overlay 未采集真实手柄，启用这些操作需要宿主通过 `NavigationInput` 接入
`GamepadState`，并在转换为 egui Key 前安装 `EngagementPlugin` 保留来源与路由。

键盘 Esc 先由文本编辑、子菜单和弹层处理，剩余的 Esc 关闭验证页；手柄 B 另有退出
Engagement 区域的步骤。关闭确认框或菜单后恢复入口焦点，长按手柄确认或取消不会
连续跨层。F8 或页面关闭按钮会清除弹层、通知和输入焦点；重新打开保留文本、
列表选择和滚动位置。这些数据仅存在于当前进程内存中。

公共 `DialogInteraction` 负责最外层 Modal 的关闭和入口焦点恢复，
`FocusEngagement::begin/show/navigate` 只衔接可选的手柄区域。
Modal 隔离 egui 背景交互，各区域共用页面边界，不为每个框创建 Modal。
游戏输入仍由调用处选择 `mhf-overlay::InputPolicy`：
隐藏时鼠标和键盘都穿透，显示期间持续阻断鼠标和键盘，即使当前控件已失焦或鼠标
位于内容面板外。策略变化期间，已按下的鼠标按钮和按键会保持原来的接收方直到松开。
其他调用方可分别选择 `Auto`、`Block`、`PassThrough`；通知对后方 egui 控件的穿透则
通过 `Notifications::set_pass_through` 配置。

MHF 输入适配层拦截 [`GetDeviceState`](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/ee417897(v=vs.85))
返回的鼠标和键盘状态，使用与窗口消息相同的策略和原始按键归属。鼠标支持 16/20 字节
标准状态，捕获时过滤按钮、相对移动和滚轮；键盘使用 256 字节扫描码状态。设备类型和
数据长度必须匹配，手柄、未知格式和失败调用保留原样。Hook 地址从运行时设备虚表
获取，不依赖游戏 DLL 的固定地址，并在 `mhDLL_Main` 返回后先于 D3D9 Overlay 卸载。

验证页只调用通用组件，不读写游戏数据。D3D9 渲染、窗口输入和游戏本身的输入接收
需要在实际游戏环境中检查；手柄、IME 组合输入以及 RawInput 的输入
仲裁仍由宿主负责。
