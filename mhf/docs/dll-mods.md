# DLL Mod 的分发与组合

包管理、宿主适配、公开 ABI、Rust SDK 与组合示例已实现。唯一 launcher 通过 Mod 提供登录或调试启动；真实 Windows HD 游戏交互仍需验证。总体职责见 [Mod 系统](mod-system.md)。

通用游戏宿主由 [`mhf-game`](../crates/runtime/game/README.md) 提供，接收 `LaunchConfig`、已解析的包和 Factory 后调用 `run`。
应用组装包，Login 持有登录网络、UI 与凭据，Debug 持有临时猎人启动；基础 ABI 和游戏内核不依赖这些实现。

## 包与唯一版本来源

目标固定为 HD `mhfo-hd.dll`、`i686-pc-windows-msvc`，清单不设 `target`。Windows 加载器报告实际 DLL 加载错误，不另写 PE 架构预检。

```text
mods/
  example.counter-consumer/
    1.0.0/
      mod.toml
      mod.dll
      assets/
```

```toml
schema = 1
id = "example.counter-consumer"
name = "计数器 Rust 调用示例"
version = "1.0.0"
kind = "native"
entry = "mod.dll"

[dependencies]
"example.counter" = "^1.0"
```

`mod.toml` 的 `version` 是外部 Mod 包的唯一运行时版本来源。宿主无需加载 DLL 即可发现、显示和解析版本。查询入口不重复返回包版本，不比较清单与 DLL 的两份值。

`semver::Version` 读取包版本，`semver::VersionReq` 读取依赖范围和用户选择，直接使用 `serde`；不另写版本解析器。解析满足全部消费者约束的唯一候选，支持传递依赖回溯，报告缺失、明确关闭、版本冲突、循环和相同 ID／版本的重复来源。

`dependencies` 是唯一依赖声明，不另设 `host_services`、所需接口列表或接口版本清单。宿主通过 `mhf_mod_query_v2` 获取生命周期入口，再按 `example.counter.v1` 等标识查询提供方接口；包版本始终来自清单。

内置实现使用 `Source::Builtin`，由 `BuiltinCatalog` 提供元数据，再由[应用工厂](../crates/apps/launcher/src/builtins.rs)构造，当前内置版本为 `1.0.0`。数据包使用 `kind = "data"` 且不设 `entry`；宿主为其提供 `mhf.data.v1` 资源访问接口，无需空 DLL。

## 公开 C ABI 与 safer-ffi

- Rust 定义：[`mhf-mod-api`](../crates/runtime/mod-api/src/lib.rs)。
- 生成的 C 头文件：[`mhf_mod.h`](../crates/runtime/mod-api/include/mhf_mod.h)。
- Rust 基础包装：[`mhf-mod-sdk`](../crates/runtime/mod-sdk/src/lib.rs)。

DLL 导出名称 `mhf_mod_query_v2`，返回 `ModV2` 表；Host 使用 `HostV2`，`GameInfoV2` 只包含游戏模块和生命周期阶段。
基础生命周期、Host、Hook、Data 和 Hook 诊断结构使用 `safer_ffi::derive_ReprC` 描述 C 布局并生成头文件。函数及回调采用 Windows x86 C 调用约定；C 示例通过 `.def` 确保入口名称可直接查询。领域能力接口保持各自现有的 v1 标识。

游戏功能和 Counter 用一份公开数据类型及 `#[derive_ReprC(dyn)]` trait 定义接口，由 safer-ffi 生成 C vtable。`*Table` 只是 `VirtualPtr` 的类型别名，没有额外对象外壳。提供方以 `Rc` 持有独立分配的 Service 或接口表，保持发布地址与后续 `&mut Mod` 生命周期借用分离。C 调用通过 `vtable` 和 `ptr` 完成，Rust 直接使用相同强类型及必要便利方法。

跨 DLL 使用生成的 C 布局，数值类型直接对应 Rust 的 `u8`、`u16`、`bool`、`usize` 等；`usize` 对应目标平台的 `size_t`。功能接口用 `str::Ref`、`slice::Mut` 和引用表达借用，C 调用需提供有效非空指针，即使切片长度为零。基础 Host 仍使用其 `Str` 和文本复制协议。字体名称直接借用；配置读取使用调用方缓冲和 `required` 长度，不含 NUL。不传递标准库 `String`、`Vec`、Rust 原生 trait object 或 `egui::Ui` 的布局。

`HostV2` 包含当前实例的 context、日志、配置 TOML、资源目录、last_error、接口注册、依赖查询、game_info 及 Hook 函数表。游戏模块基址属于宿主借用；Mod 不得持有独立游戏 DLL 引用越过 detach。架构和游戏地址由固定目标及所属 Mod 实现处理。

状态 0 表示成功。Host 错误可通过 `last_error` 读取；SDK 将失败转成 `Result`。Rust 生命周期适配和 UI 渲染闭包捕获 panic，错误不会展开越过 C 边界；其他语言同样必须自行阻止异常跨界。访问违规或进程终止不属于可恢复错误。

Host 查询返回的所有接口均为借用。消费者只保存借用的表指针，不复制 `VirtualPtr` 作为自有对象，也不调用 `vtable.release_vptr`。提供方对象的释放由宿主的实例销毁顺序控制，不能通过虚拟对象绕过依赖生命周期。UI 提供方的 `UiTable` 借用更短，只在当前渲染回调内有效，不得保存或释放。

## 头文件生成

Rust 定义是公开 C 接口的唯一来源。构建启动应用、Counter 提供方和 Hook 示例时，各自的 `build.rs`
自动把头文件生成到该 package 的 `OUT_DIR/include`。通用游戏内核不承担领域头文件聚合。
仓库中的以下五个文件是供审查和 C 消费者使用的快照，不手工编辑：

| 生成文件 | Rust 定义 |
| --- | --- |
| [`mhf_mod.h`](../crates/runtime/mod-api/include/mhf_mod.h) | [`mod-api`](../crates/runtime/mod-api/src/lib.rs)：生命周期、Host、Hook 和基础值 |
| [`mhf_game.h`](../crates/apps/launcher/include/mhf_game.h) | 领域 API：[Config](../crates/mods/config/src/api.rs)、[Font](../crates/mods/font/src/api/mod.rs)、[Quest](../crates/mods/quest/src/api/mod.rs)、[DebugTools](../crates/mods/debug-tools/src/api/mod.rs)、[UI](../crates/mods/ui/src/api/mod.rs) |
| [`mhf_data.h`](../crates/runtime/mod-host/include/mhf_data.h) | [`mod-host/src/data.rs`](../crates/runtime/mod-host/src/data.rs) |
| [`counter.h`](../examples/mods/counter-sdk/counter.h) | [`counter-sdk/src/lib.rs`](../examples/mods/counter-sdk/src/lib.rs) |
| [`probe.h`](../examples/mods/hook/probe.h) | [`hook/src/probe.rs`](../examples/mods/hook/src/probe.rs) 的诊断表，由库和构建脚本共用 |

[`apps/launcher/build.rs`](../crates/apps/launcher/build.rs) 在构建依赖中启用各领域的 `headers`，
调用 `api::define_header`，并与基础协议和 `mhf-mod-host` 的 Data 定义聚合，
生成 `mhf_mod.h`、`mhf_game.h`、`mhf_data.h`。
[`counter-provider/build.rs`](../examples/mods/counter-provider/build.rs) 生成 `mhf_mod.h` 与 `counter.h`；
[`hook/build.rs`](../examples/mods/hook/build.rs) 生成 `mhf_mod.h` 与 `probe.h`。
这些生成依赖只需要领域 API，不编译领域 provider；所有输出直接放在各 package 的 `OUT_DIR/include` 中。

普通 `cargo build` 只更新构建目录中的文件。各 package 的 `tests/headers.rs` 检查构建产物与仓库快照
是否逐字节一致，差异会使测试失败。以下命令从仓库根目录运行；跨平台执行 Windows 目标测试时，
需配置 Windows x86 链接器，并用 `CARGO_TARGET_I686_PC_WINDOWS_MSVC_RUNNER` 指定可用的 Wine 命令：

```sh
cargo test --offline --manifest-path mhf/Cargo.toml -p mhf-launcher --target i686-pc-windows-msvc --test headers
cargo test --offline --manifest-path mhf/examples/mods/Cargo.toml -p example-counter-provider --target i686-pc-windows-msvc --test headers
cargo test --offline --manifest-path mhf/examples/mods/hook/Cargo.toml --target i686-pc-windows-msvc --test headers
```

修改 Rust 接口后，给相应测试命令加上 `MHF_UPDATE_HEADERS=1` 环境变量，显式更新它负责的仓库快照，
再审查生成差异。给上述三个测试命令都设置该变量，依次运行即可更新全部五个文件。

打包时给相应测试命令设置 `MHF_HEADERS_EXPORT_DIR=/path/include`：测试先检查快照一致性，
通过后将已构建的头文件复制到指定目录。三个测试使用同一个导出目录即可汇集全部五个头文件；
Counter 和 Hook 的基础 `mhf_mod.h` 与启动应用使用同一定义。这两个环境变量均由测试读取，
普通构建不会改写仓库快照或额外导出文件。

受 `safer-ffi 0.1.13` 派生宏在宿主侧统一 feature 的限制，仅在构建依赖上开启底层 `headers`
会导致交叉编译的目标代码缺失相关定义，因此底层 `safer-ffi` 的头文件元数据统一启用。
各 API crate 自身的 `headers` feature 用于开启构建端的生成函数；实际文件生成由 `build.rs` 执行。

## 创建、发布与退出

`DllMain` 不安装 Hook、不启动工作线程、不初始化 Overlay。宿主取得函数表后，在明确阶段调用：

| ABI 字段 | 含义 |
| --- | --- |
| `create(host, out_instance)` | DLL 内分配实例，保存宿主借用；此时依赖尚未完成准备 |
| `prepare` | 可选；加载游戏前准备资源和注册接口 |
| `check` | 可选；游戏已加载，执行目标预检 |
| `attach` | 可选；安装功能、绑定依赖并注册接口 |
| `stop` | 可选；停止线程、队列、订阅和 UI 回调 |
| `detach` | 可选；撤销功能，归还独立游戏引用 |
| `destroy` | 在原分配 DLL 内释放实例；返回后宿主才释放该 Mod DLL |

实例按依赖顺序创建，各阶段也按提供方在先的顺序执行。`prepare` 或 `attach` 成功后发布该阶段注册的接口；失败不发布部分接口。调用方只可获取清单中已声明依赖的已发布接口。普通运行调用直接走取得的函数表，无逐次许可或租约登记。

启动 Mod 在 prepare 发布 `LaunchApiV1`：普通入口为 `mhf.launch.v1`，默认入口为 `mhf.launch.fallback.v1`。
宿主优先选择普通入口，没有时才使用 fallback；有效层必须只有一个提供方。
回调在游戏加载前借用 `LaunchTargetV1` 中的 `LaunchParams32`／`GlobalData32`，不得保留指针；
返回 `CANCELLED` 正常取消启动。Login 使用 fallback，因此启用 Debug 会自动接管启动。

退出先停止消费者，再停止提供者。原生入口停止、Hook 禁用／排空／移除和退役状态释放分阶段进行；清理失败保留实例、依赖、DLL 和缓冲。宿主内部还有 `prepare_release` 用于现有内置适配器，不增加一套 DLL 回调。完整顺序见 [生命周期](mod-system.md#生命周期)。

## Rust Mod 与提供方 SDK

Rust Mod 实现 `Mod<'host>`，通过 `export_mod!` 生成入口和 C 生命周期适配。Host 和依赖查询在生命周期线程上执行；返回的接口借用受实例生命周期约束，线程能力由接口的 `Send`／`Sync` 定义决定。安全操作建立在提供方遵守公开 ABI 与宿主生命周期的契约上。

```rust
use mhf_mod_sdk::{Host, Mod, Result, export_mod};

struct ExampleMod<'host> {
    host: Host<'host>,
}

impl<'host> Mod<'host> for ExampleMod<'host> {
    fn create(host: Host<'host>) -> Result<Self> {
        Ok(Self { host })
    }

    fn attach(&mut self) -> Result<()> {
        self.host.log(mhf_mod_sdk::LogLevel::Info, "Mod 已安装");
        Ok(())
    }
}

export_mod!(ExampleMod);
```

功能接口由所属领域 crate 的 `src/api` 定义，包含公开类型、可导出的 trait、Host 绑定和便利方法。
例如 `mhf_quest::Snapshot` 直接用于 Rust 返回值及生成的 C 结构，`QuestTable` 直接由 `QuestApi` 生成；
不维护第二份字段布局或整数扩宽转换。带数据枚举按语义处理：Debug 的 Rust 命令分派到各个强类型方法，
配置注册使用公开的节定义与类型。字符串复制、状态转 `Result` 和回调注销保留必要适配。
基础 `Host::game_info` 返回 `GameInfo`，阶段和可选模块分别为 `Phase`
与 `Option<GameModule>`；日志用 `LogLevel`，错误用 `ErrorKind`。基础 SDK 不理解业务表语义。
原始接口注册、Hook 指针与 `host_from_raw` 分别在 `host` 和 `hooks` 模块中公开。
基础生命周期 C 协议由独立 `mhf-mod-api` crate 定义；业务接口不再拆分 API／ABI 镜像。

领域 crate 默认提供 API，`provider` feature 增加同一领域的 Service、内置 Module 和具体实现。
应用 [`builtins.rs`](../crates/apps/launcher/src/builtins.rs) 组装内置 Factory。
内置适配使用含 `prepare_release` 的 `mhf_mod_host::Module`；上例面向 DLL 的
`mhf_mod_sdk::Mod` 不含此阶段。领域 provider 不自行调用 `export_mod!` 或生成新的 DLL 入口。

## Mod 调用另一个 Mod

真实可构建示例位于 [`examples/mods`](../examples/mods/README.md)：

- `example.counter` 实现 `CounterApi`，通过生成的 `CounterTable` 公开按值快照与原子计数增加。
- `example.counter-consumer` 声明依赖，通过提供方 `example-counter-sdk` 调用。
- `example.counter-c` 使用相同依赖，只包含 C 头文件直接调用。

Rust 消费者在 `attach` 中执行：

```rust,ignore
let counter = example_counter_sdk::Counter::bind(self.host.dependencies())?;
counter.add(1)?;
let snapshot = counter.snapshot();
```

无 SDK 的 C 消费者通过 `host->dependency` 取得同一表：

```c
const CounterTable *counter = (const CounterTable *)table;
CounterSnapshot snapshot = counter->vtable.snapshot(counter->ptr);
/* 使用 snapshot.count；counter 仍属于提供方。 */
```

提供方实例持有函数表及虚拟对象，保持它们有效直至消费者销毁；消费者不得复制虚拟对象取得所有权或调用 `release_vptr`。快照复制到调用方，ABI 不共享分配器。例子的增加操作溢出时返回错误且不修改计数。两个消费者同时启用时分别增加 1 和 10，最终计数为 11。

公开接口位于各[领域 crate](../README.md#领域-api-与提供方) 的 `src/api`，游戏能力的 C 声明聚合在
[`mhf_game.h`](../crates/apps/launcher/include/mhf_game.h)：

| 提供方 | 接口 | 已有能力 |
| --- | --- | --- |
| `mhf.config` | `mhf.config.v1` | 节注册、默认与固定值、TOML 读写及声明式 INI 映射 |
| `mhf.base` | `mhf.font.v1` | 字体 family 字符串 |
| `mhf.base` | `mhf.quest.v1` | 任务 ID、会话初始化状态、任务缓冲大小快照 |
| `mhf.base` | `mhf.quest.control.v2` | 游戏线程任务验证、重启、重置、任务替换准备与范围查询 |
| `mhf.base` | `mhf.quest.launch.v1` | 校验并准备调用方提供的原始任务字节 |
| `mhf.debug` | `mhf.debug-tools.v1` | 调试快照与游戏线程命令队列 |
| `mhf.base` | `mhf.ui.v1` | Panel 注册／注销，label、button、checkbox 控件 |

Quest 的实现位于 [`mods/quest/src/provider`](../crates/mods/quest/src/provider/mod.rs)，公开包装为 `mhf_quest::{Quest, QuestControl}`。
Quest 作为 Base 组件在 prepare 发布快照、控制与本地启动接口。Debug 调用 `prepare_local` 后，Base 才在 attach 安装任务 Hook；普通 Login 不激活该组件。

临时猎人角色字段由 Debug 的启动回调写入，登录数据由 Login 写入；游戏宿主只管理共用启动内存。

`mhf_quest::Snapshot::hunter_initialized` 同时是 Rust 与 C 的快照字段，表示离线会话已初始化临时猎人，重开任务不会清零，不能作为当前任务或地图就绪标志。

`mhf_quest::MonsterSpawn`／`prepare_monster_spawn` 准备任务替换中的资源种类、出生记录和猎人起始区，返回当前任务替换内的记录偏移，不直接创建或操纵运行中怪物。重置或再次准备后应重新取得偏移。任务二进制处理在 [`quest/provider/binary.rs`](../crates/mods/quest/src/provider/binary.rs)，实时怪物控制属于 DebugTools。

Debug 原生状态通过 Base 的公开 Quest 控制接口访问任务会话。控制接口的游戏变更方法保留为 `unsafe`；外部 UI 通常使用 Debug 命令队列，成功仅表示入队，执行结果通过快照或游戏消息观察。快照中的怪物使用可导出的 `TaggedOption<u8>`，Rust 的 `monster()` 访问方法返回普通 `Option<u8>`。

`mhf_ui::UiHost` 从 `mhf.base` 的 `mhf.ui.v1` 注册面板，回调中的 `UiTable` 只在该次调用内有效；`Panel::close` 成功后已排空回调，失败应在 `Mod::stop` 中传播，让宿主保留消费者 DLL。回调不能同步注销自身。当前完整 egui 调试窗口要求内置 Base，不跨 DLL 传递 egui 对象。
Base 内部包含 Font、UI、Geometry 和 Quest；各组件保留自己的代码边界，游戏文本保持原生处理。

Unicode 和 Translation crate 暂未接入应用，当前运行清单与应用头文件聚合不包含它们。

数据 Mod 的 [`mhf.data.v1`](../crates/runtime/mod-host/include/mhf_data.h) 提供资源根路径及相对文件读取。它是通用资源入口。

## 配置、导入与导出

包清单不保存用户设置。运行选择和参数只在 `mhf.toml`：

独立 `mhf.config` 提供通用配置服务，Base 提交游戏字段与 INI 映射，
Login 自己注册、读取并解析 Sign 配置。服务不反向依赖消费者 schema，
注册与读写示例见 [Config](../crates/mods/config/README.md#注册与访问)。

```toml
[mods]
directory = "mods"

[mods."example.counter-consumer"]
enabled = true
version = "^1.0"

[mods."example.counter-consumer".settings]
label = "计数器"
```

游戏启动器与管理器均将配置中的相对 `directory` 按调用目录解析，并在切换游戏工作目录前确定路径。DLL 使用规范化绝对入口路径加载，私有依赖搜索包括包目录和 Windows 默认安全搜索目录。

独立管理器由 [`mhf-mod-manager`](../crates/apps/mod-manager/README.md) package 提供，使用
[`mhf-mod-package`](../crates/runtime/mod-package/README.md) 的共用内置元数据和 semver 解析器，
检查配置中明确启用项及其声明依赖。工具可放在 PATH 中任意位置，
在要管理的运行目录执行 `mhf-mods` 打开图形界面；显式子命令继续使用 CLI：

```sh
mhf-mods
mhf-mods list
mhf-mods import counter-pack.zip
mhf-mods enable example.counter-consumer --version "^1.0"
mhf-mods export selected-counter.zip "example.counter-consumer@=1.0.0"
mhf-mods disable example.counter-consumer
```

`mhf-mods` 默认读取命令启动时当前目录中的 `mhf.toml`，缺省 Mod 目录为该目录下的 `mods`。
配置中的相对 `directory`、显式 `--config`／`--mods-dir` 和 ZIP 相对路径均使用同一当前目录，
不随配置文件或工具的位置改变；绝对路径直接使用。
配置和 Mod 目录在启动时确定，配置文件必须已存在；界面中的文件选择器用于 ZIP 导入／导出。

ZIP 内使用 `mods/<id>/<version>/*`，整合包的 `pack.toml` 记录已解析的精确版本。单包使用同样目录结构，可省略 `pack.toml`。导入验证路径和清单后发布到 Mod 根目录，不覆盖已有版本，不自动修改启用设置。导出记录不是第二份运行配置。

界面可设置自动／启用／禁用及指定版本，预览依赖后保存或撤销，并在后台导入、导出 ZIP。
GUI 导出已保存配置中明确启用的项及其完整依赖，与不指定 ID 的 CLI 导出一致；需要的内置依赖记录精确版本。
两者均拒绝覆盖已有 ZIP。启动器应用默认启用项，并按声明补齐依赖；导出包含默认项的实际会话选择使用：

```sh
mhf-launcher --config mhf.toml --game-dir GAME --export-modpack selected.zip
```

该命令不调用登录或调试启动接口；导出内容包含内置 Mod 的精确版本记录和所选外部包，使用包库的 `export_archive`。
管理器也不加载 Mod DLL 或游戏，且不会把尚未解析的版本范围作为精确结果导出。详细路径语义和选项见 [管理器文档](../crates/apps/mod-manager/README.md)。

## 验证与限制

示例提供 Windows x86 构建和打包脚本，以及 C 宿主 `abi_smoke.c`。macOS 动态库和 Windows/Wine [`ModHost smoke`](../crates/runtime/mod-host/examples/smoke.rs) 覆盖 C／Rust 提供方与消费者调用：检查两种消费者共享计数器得到 11、自有目标的 Hook 生效、排空并恢复，并在同进程重复执行。构建与运行方式见 [Hook 示例](../examples/mods/hook/README.md)。

测试覆盖游戏库、启动器、SDK、元数据、ZIP、配置编辑、Overlay、Geometry 和 Hook；头文件一致性由上述 Cargo 集成测试检查。
通用 HD DLL 测试检查最小启动提供方注入及游戏 DLL 加载／卸载，调用真实 DllMain，不调用游戏主入口；具体 Mod 安装由各组件测试覆盖。

启动器的 `--list-mods`、`--export-modpack` 与管理器读取同一配置具有已有验证覆盖。游戏内 Panel 操作、输入／IME、开图、游戏线程命令及实玩退出仍待交互验收。首版没有原生 Mod 热卸载、自动组合任意内存补丁或通用 detour 链，实际边界见 [Hook 冲突规则](mod-hooks.md#冲突规则)。
