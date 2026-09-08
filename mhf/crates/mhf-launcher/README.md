# MHF launchers

同一个 package 提供两个独立 bin：`mhf-launcher` 负责登录和角色选择，
`mhf-debug-launcher` 直接启动离线任务。`login`、`debug` 特性分别控制两条入口；
默认仅启用 `login`。独立调试构建不编译或链接登录界面、eframe/wgpu、Sign HTTP
客户端与凭据存储，也不要求 `[sign]` 配置。游戏内调试 Overlay、字体、汉化、
几何扩展及 INI 配置桥接仍保留。

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
地位相同，空 JSONL 表示没有覆盖。只有显式配置 `[translation]` 才会应用翻译覆盖；
省略该 section 时仍将原始资源转为 UTF-8，显示原文。资源结构和合法 key 由 resource layout 定义，
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
`mhf.toml`；其他 INI 请求仍转发给原始 Win32 API。目标 INI 的名称和值直接使用 UTF-8，
并把领域字段映射为游戏使用的大写 INI 名称、
`0`/`1` boolean 和数字枚举。游戏写回时执行反向映射；TOML 会被重新格式化，原
注释不保证保留。

INI、汉化/GDI、D3D9 和 DirectInput 分别持有自己的 hook 组，共用 `mhf-hooks` 的 MinHook 生命周期
管理。创建失败会回滚本组已创建的目标；回调状态就绪后才逐个启用，不使用进程级的全局
启停操作。退出时先停用目标并等待 Rust 回调及原函数调用结束，再移除 trampoline；
清理错误会返回给启动流程。汉化裸汇编仍使用原有 ABI，要求游戏调用线程在
`mhDLL_Main` 返回时已结束；游戏 DLL 和翻译缓存保留到汉化 hook 清理完成。

每次启动都保持 `mhf_mutex_number = 0`，并用当前进程 ID 创建独立的
`MHF_MASTER` 与 `MHF_MASTER_READY` 互斥量，因此允许多开。

HD 启动会安装独立的 [`mhf-geometry`](../geometry/README.md) 模块，将模型加载后的
顶点索引、三角条带和绘制批次计数扩展为 32 位，并更新四条原生绘制路径。原有 FMOD
资源无需转换；模型分配和释放仍由原生生命周期管理。模块目前针对已核对的 ZZ HD DLL，
不匹配的指令签名会在启动前报错；标准画质客户端不安装此扩展。

## 离线任务调试

直接运行 `mhf-debug-launcher`，在内存中生成临时猎人，默认加载极驱迅龙使用的
**古迹大地图**，从营地 460 出生。任务 BIN 内嵌在启动器中，无需另行下载或提供任务文件；
加载时将任务标题、目标、成败条件、委托人与说明替换为中文 UTF-8 文本。
也可用 `--quest <BIN>` 加载指定任务，保留该文件的文本和出生配置。
目前实现针对当前 ZZ HD 客户端，接受编号 40000 以上的活动任务；文件可为原始 BIN
或 JKR 类型 3 压缩文件，解压内容不得超过游戏的 32 KiB 任务缓冲区。

在项目的开发环境中，例如：

```sh
rtk proxy direnv exec /Users/wzk/Projects/RustProjects/shrimpman mhf-debug-launcher
```

传入自定义 BIN 时，当前 macOS Wine 环境使用 `Z:\...` 路径；原生 Windows 使用本地路径。
游戏目录和 TOML 配置沿用 `--game-dir`、`--config` 及开发环境配置。

游戏内调试窗口默认打开，按 **F7** 显示或隐藏。**F8** 仍是现有组件验证窗口。
窗口随游戏画面限制最大尺寸，内容在窗口内滚动；下拉列表按上下剩余空间限制高度。

- **装备**：按武器种类或防具部位筛选、搜索名称或编号，点击“换装”后重载当前任务。
  装备名称与编号来自已加载的 DAT；操作只修改当前进程中的临时角色。
- **实际招式**：选择任意招式来源武器，点击“触发”调用原生招式状态机。
  跨武器时保留当前装备记录，在重载任务期间切换本地猎人的招式分类，让后续原生加载器
  载入对应的动作资源；加载完成后触发选中的招式。“招式跟随装备”恢复正常对应关系。
  招式列表使用客户端动作目录中的编号，未确认名称的招式不推测名称。
- **状态**：显示装备类型、招式来源、状态类别与编号、动作阶段、动画编号、当前帧和位置。
- **一键换区**：窗口顶部选择当前地图内的目标区域后点击“一键换区”，无需走到出口。
  默认古迹地图可在营地 460 与战斗区 461 间切换；优先使用任务入口的落点，没有返回
  入口的区域使用客户端原生出生点。变身时会携带所选怪物，并在载入后恢复操控。
- **怪物变身**：从完整中文种类列表选择，点击“变身并操控”后重载当前地图、加载所选
  怪物的模型与动作资源，保留原任务怪物，再单独生成并接管一个受控实例。无需场上已有
  该种类；即使与任务目标同种，也不会接管或替换目标。受控实例不注册为主任务目标。
  其他怪物保留 AI，并通过同步到怪物位置的玩家代理锁定你。猎人只在绘制时隐藏；
  攻击玩家代理的命中交给受控怪物的身体与伤害状态处理，受控怪物的攻击可命中其他
  怪物，并排除自己及玩家代理。敌对命中还会放行身体部位对怪物攻击类型的过滤，
  使用独立受击数据副本，保留原来的形状、部位和免疫状态。伤害和受击反应沿用原生逻辑。
  窗口顶部的“交战状态”显示双方原生 HP、命中检查次数、确认命中次数和最近一次
  实际扣血；命中计数与扣血分别记录，以区分受击判定和后续生命值结算。
  选择种类后立即显示其真实招式目录，支持筛选、直接触发和绑定快捷键 1–4；
  尚未变身时，触发招式会先加载对应怪物，再执行所选招式。
  点击游戏区域使焦点离开调试窗口后，W/S 相对镜头前后移动、A/D 相对镜头左右移动、Q/E 调试升降、
  Shift 加速；窗口可以保持打开。调试窗口或其他 Overlay 控件有键盘焦点时暂停操控。
  变身镜头默认拉远至至少 1200、俯视 20°，可在“操控设置与说明”里调整距离和
  垂直角度（−60° 至 80°，正值俯视、0° 平视、负值仰视）；恢复猎人后使用原生镜头。
  进入任务原生出口的水平范围可换区；在怪物碰撞处理前探测入口，站在跳崖入口上方
  也会直接触发，不要求继续移动、面朝入口或处于特定高度。入口下方不会反向触发；
  多个入口上下重叠时选择最近的下方入口。目标区域、落点与朝向来自任务入口记录。
  加载期间暂停操控，完成后在新区的原生落点继续使用
  所选怪物，并重新应用镜头距离与垂直角度。怪物自动朝移动方向转身，斜向移动不加速；
  移动方向取镜头朝向在地面上的投影，调整俯仰角不会改变移动速度。
  猎人模型隐藏期间仍同步区域、朝向，并更新原生地图坐标，使地图位置箭头跟随怪物。
  “原生选招”位于折叠的操控设置中，R 可执行一次选招，不是显示招式目录的前置步骤。
  Backspace 或“恢复猎人”重载原始任务，恢复原有装备和猎人操控。
  变身任务副本只存在于内存，原始 BIN 不变。客户端最多同时载入 6 种怪物资源；
  资源槽已满时选择已载入种类，或恢复猎人后换一个任务。特殊巨型怪物、机关及部分形态仍依赖
  对应地图与任务脚本；当前招式目录只收录能从客户端明确提取或运行中观察到的编号。
- **任务控制**：“重开任务”重新加载同一文件；“结束调试”通知原生主循环退出。

UI 只发送命令并读取快照，所有角色、装备、动作和场景变更均在原生任务调度线程执行。
离线 hook 随此次启动安装，并拦截 `connect` / `WSAConnect`；普通在线启动不安装它们。
跨武器招式混用属于实验调试功能：此实现没有为每种混搭补齐专属武器计量槽、弹药或附属对象，
这些调试行为尚未逐项进行游戏内验证。

## 代码结构

- `src/abi.rs`：32 位 `repr(C)` 结构、函数签名和布局断言。
- `src/model.rs`：定义共用的游戏 `MhfConfig` 和启动 profile。
- `src/launcher.rs`：负责领域模型到 ABI 的映射及 Win32 启动流程。
- `src/debug/`：离线任务解压、原生调试 hook、游戏线程命令与快照、内嵌调试窗口。
- `src/text/`：接管原生 UTF-8 分词、编辑、标记展开、换行和字形缓存；光标与容量仍
  以字节计，排版使用 Unicode 显示列数，Win32 绘制和剪贴板使用 UTF-16。IME 提交与
  普通 `WM_CHAR` 都按 Unicode 输入，删除、选区和滚动不会拆开 UTF-8 字符。
  `printf` 的字符串宽度按显示列数补白；精度保留 C 的字节读取上限，并在 UTF-8
  边界截断。整数、浮点和缓冲容量语义仍由游戏 CRT 执行。行内字串构造、倒计时与
  按索引取空白的布局表分别保留原调用约定。
  全角转换根据已核对的原生缓冲区容量写入；很小的数字栏若放不下全角 UTF-8，会
  保留完整半角数字。截图文件名使用 UTF-8，并通过宽字符接口创建目录和保存图片。
- `src/overlay/input.rs`：过滤 MHF 通过 DirectInput 读取的鼠标、键盘状态，复用
  Overlay 调用处的输入策略；`input/polling.rs` 保留每个按键从按下到松开的接收方。
- `src/localization/`：在 DAT/INF/PAC/JMP/GAO/SQD/RCC/MSX 完成原生指针重定位、
  首次消费或复制前，按 layout 遍历文本槽；同时遍历 TLK 的全部 section 与记录。译文直接引用 EXE 内 NUL 结尾的
  UTF-8 常量，未翻译原文按资源来源的 CP932/949/950 转为稳定存储的 UTF-8；同时更新
  PAC 提前缓存的文本指针，后续别名自然继承新指针。`native/layout.rs` 按记录步长
  解析 GR/HR 表及房间指针字段，`native/bindings.rs` 只保留零散编译常量的引用绑定；
  两者统一接入 JSONL 翻译键、语言选择和缺失译文策略。
  省略翻译配置也会转换原文；运行时不解析 JSONL，不使用虚拟字形码表。
- `src/bin/mhf-launcher/http/`：Sign HTTP 客户端及按 API 命名空间组织的请求、响应
  模型。
- `src/bin/mhf-launcher/ui/`：按 Elm 结构组织状态更新、界面渲染和 eframe 适配。
- `src/runtime/config.rs`：共用 `[translation]` 和强类型
  `MhfConfig`，并将后者映射到 TOML 持久化格式；原始 `toml::Table` 只封装在私有
  `Store` 中；只有正常启动入口才读取并校验 `[sign.http]`。
- `src/runtime/ini_hook.rs`：把 Win32 Profile API 代理到 TOML。
- `src/runtime/mod.rs`：共用目录解析、字体注册、INI Hook 和启动清理。
- `src/bin/mhf-debug-launcher/main.rs`：解析离线任务参数，直接启动调试会话。
- `src/sign.rs`：仅 `login` 特性编译的 Sign 会话与角色类型。
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

游戏文本和 Sign/Entrance 协议一起使用 UTF-8，联网启动需要配套的 Shrimpman 服务端；
离线调试不连接这些服务。固定
字段保留原字节容量，例如 Sign 角色名字段为 16 字节（含结尾 NUL），不会为了编码
迁移扩展协议包。原生游戏目录参数仍限制为 ASCII；更改文本编码不改变这个路径约束。

## 构建和启动

DLL 均为 32 位，因此必须构建 i686 版本。原生 Windows 安装 MSVC 构建工具和
Windows SDK 后使用 Cargo，直接运行生成的 EXE，不需要 cargo-xwin 或 Wine：

```text
cargo build -p shrimpman-mhf-launcher --bin mhf-launcher --release --target i686-pc-windows-msvc
cargo build -p shrimpman-mhf-launcher --bin mhf-debug-launcher --no-default-features --features debug --release --target i686-pc-windows-msvc
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
mhf-debug-launcher.exe --config mhf.toml --game-dir D:\mhf
mhf-debug-launcher.exe --quest quest.bin --config mhf.toml --game-dir D:\mhf
```

在 macOS/Linux 的仓库根目录进入 flake 开发环境后，用开发命令构建和启动：

```sh
nix develop --impure
mhf-build
mhf-launcher
mhf-debug-build
mhf-debug-launcher
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
nix run .#mhf-debug-build
nix run --impure .#mhf-debug-launcher
```

`mhf-build` 和 `mhf-debug-build` 分别只构建自己的 bin，显式选择 `login` 或 `debug` 特性，使用普通 `cargo build`。Flake 提供 LLVM 和 x86 Windows SDK/CRT，
并设置 `i686-pc-windows-msvc` 专用编译、归档和链接环境变量；进入 `nix develop`
后也可在 `mhf/` 直接执行 `cargo check --workspace --all-features --all-targets`。
RustRover 需继承该开发环境，再重新加载 Cargo 项目，无需 Cargo wrapper。
SDK 由 Nixpkgs 的 xwin 构建步骤准备，项目的 Nix 配置接受其 Microsoft 软件许可。
当前 flake 的输出仅覆盖
macOS/Linux；原生 Windows 使用上面的 Cargo 和 EXE 命令，WSL 使用 Linux 输出。
Cargo 会判断构建输入是否变化并复用未变化的产物。游戏目录由 `development.mhf.gameDirectory`
提供。Nix 使用 `$PROJECT_STATE/config/mhf.toml` 作为可写配置，默认路径为
`.state/config/mhf.toml`；同目录的 `mhf.generated.toml` 保存上次 Nix 原始生成内容。
启动时按生成内容与快照比较：内容相同则复用可写配置，保留游戏回写的设置；内容变化时
刷新两份文件，并重置此前游戏回写的设置。文件名固定，不再按哈希积累历史配置。
设置 `MHF_CONFIG` 可使用独立管理的配置，其相对路径以 `mhf/` 为基准；脱离 Nix 时
使用 `mhf/mhf.toml`。
启动方式独立选择：macOS/Linux 默认使用 Wine；检测到 WSL 的 Windows 互操作
已启用时直接执行 EXE，通过 `wslpath` 转换配置和游戏目录，并通过 `WSLENV`
转发 `MHF_*` 环境变量。显式设置 `development.mhf.runner`
可指定 Wine 可执行文件，设为空字符串则直接执行 EXE；`WINEPREFIX` 默认为
`$PROJECT_STATE/wine`，公共状态目录默认是仓库的 `.state/`。原生 Windows 直接执行 EXE。
MHF 配置副本和 Wine 默认环境仅在启动器运行时准备，进入开发环境或编译时不会初始化。
正常启动器的 Sign HTTP 地址默认使用开发环境的 Nix 选项 `development.ports.signHttp`（53001），可通过
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
需要在实际游戏环境中检查。游戏原生输入框和 Overlay 使用同一个 IME 管理器与
输入上下文，由文本焦点选择唯一接收方。没有文本焦点时会解除窗口的输入法关联，
让按键用于游戏操作；进入输入框后重新使用保留中英文状态的共享上下文。
Overlay 有键盘输入权时，游戏输入框及其
内部软键盘命令暂停接收；切换接收方会取消原来的组合，卸载时恢复游戏原始上下文。

原生适配器根据游戏输入框的实时光标位置定位系统候选窗，将共享上下文的 Unicode
组合与提交事件写入 UTF-8 文本缓冲；egui 使用同一组 Unicode 事件。普通 `WM_CHAR`
在进入旧 ANSI 窗口过程前组装 UTF-16 代理对，避免字符被截成一个字节。
输入保留游戏原有字节容量。适配器校验对应游戏 DLL
的函数签名与布局，避免将其他版本的地址当作输入框使用。
真实输入法的选词和上屏仍需在 Windows/macOS Wine 中验证。
手柄以及 RawInput 的输入仲裁仍由宿主负责。
