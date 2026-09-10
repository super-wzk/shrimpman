# mhf-mod-sdk

每项能力只在自己的 `mod.rs` 中定义类型、方法和必要的原生适配，不再维护 `abi.rs`／`sdk.rs` 两份接口：

```text
src/
  host/mod.rs       # Host、游戏信息、注册与原生模块句柄
  error/mod.rs      # 类型化错误及状态转换
  lifecycle/mod.rs  # Mod trait、生命周期适配和导出宏
  interface/mod.rs  # Interface、InterfaceRef 与 bind
  hooks/mod.rs      # Hook 组及原生操作
```

库根继续导出 Host、Dependencies、Mod、Result 等常用 Rust 入口。业务接口直接使用
`safer_ffi::derive_ReprC` 描述其强类型值及 trait，从同一定义生成 C 类型和虚表。
根 `abi` 重导出的是独立 `mhf-mod-api` 的真实宿主 C 协议，不是一份业务 API 镜像。

## 普通 Rust API

`Host<'host>`、`Dependencies<'host>` 和 `Mod<'host>` 沿用会话生命周期。Provider SDK 接受
`Dependencies` 并在内部绑定自己的接口；普通调用方不接触函数表、C 状态码或裸指针。

```rust
use mhf_mod_sdk::{Host, LogLevel, Mod, Result, export_mod};

struct ExampleMod<'host> {
    host: Host<'host>,
}

impl<'host> Mod<'host> for ExampleMod<'host> {
    fn create(host: Host<'host>) -> Result<Self> {
        Ok(Self { host })
    }

    fn attach(&mut self) -> Result<()> {
        let game = self.host.game_info()?;
        self.host.log(LogLevel::Info, &format!("当前阶段：{:?}", game.phase));
        Ok(())
    }
}

export_mod!(ExampleMod);
```

| Rust API | 类型与语义 |
| --- | --- |
| `Host::log` | `LogLevel`：Error、Warn、Info、Debug、Trace |
| `Host::game_info` | `GameInfo { module, mode, phase }` |
| `GameInfo::module` 字段 | `Option<GameModule>`；加载前为 None |
| `LaunchMode` | Online／Offline |
| `Phase` | Prepare／Check／Attach／Running／Stop／Detach／Destroy |
| `Error::kind()` | `ErrorKind`；原始状态只在接口状态转换中使用 |
| `Host::config`／`resource_root` | 当前 Mod 的 TOML 文本／资源路径 |

`GameModule` 是使用透明表示并直接 `derive_ReprC` 的不透明句柄，内部使用 `NonNull`，原生转换入口
由 host 模块明确提供。它不保活游戏 DLL，也不保证 Mod 实例尚在时游戏 DLL 仍加载；原生操作
仍必须满足当前加载阶段与线程契约。Host 和 Dependencies 保持非 Send／Sync。
接口取得后，`InterfaceRef` 的 Send／Sync 由实际 Table 推导，包括虚拟 trait 声明的线程约束；
绑定句柄不再额外叠加线程标记。

## 原生适配与接口绑定

根模块的 `abi` 重导出 `mhf_mod_api` 定义；[`mhf_mod.h`](../mod-api/include/mhf_mod.h)
由这些 `derive_ReprC` 定义生成，包含基础值、Host、生命周期与 Hook 表。

- `host::host_from_raw`／`host_raw`：宿主句柄与原始表之间的桥接。
- `interface::Interface`／`InterfaceRef`／`bind`：Provider 对固定表布局的承诺与绑定。
- `host::register_interface`：在 prepare／attach 中登记稳定的函数表。
- `host::game_module_from_raw`／`game_module_ptr`：显式访问模块标识的原生表示。
- `error::error_from_status`／`error_status`／`status_result`：将 C 失败转换为 Rust 错误，转回时保留未知 Provider 状态。
- `hooks::hooks`：原始 Hook 组扩展；目标、detour、私有状态和 drain 仍由具体实现负责。

例如 Provider 的安全包装内部可以绑定其私有 marker：

```rust,ignore
let binding = mhf_mod_sdk::interface::bind::<CounterInterface>(dependencies)?;
```

游戏功能和 Counter 的 Provider 使用 `derive_ReprC(dyn)` 与 `VirtualPtr` 生成业务 vtable。
游戏领域 API 位于 `crates/mods/{font,ui,quest,debug-tools,translation}/src/api`，
各领域默认提供 API，`provider` feature 增加具体实现；SDK 不包含这些领域定义。
Counter 的 `Snapshot` 直接作为 trait 返回值和 C 按值返回类型，不再经过 V1 快照镜像或外壳。
`interface::bind` 返回的表仍然是宿主借用；消费者不取得虚拟对象所有权。
C 调用方不得复制其中的 `VirtualPtr` 作为自有对象，也不得调用 `release_vptr`。
Provider 和 Mod DLL 的释放顺序由宿主控制。

`Interface` 的实现需要 `unsafe` 布局契约；基础 SDK 不自动验证任意函数表语义。原始 Hook
操作也不会被泛化成声称安全的函数签名。完整示例见 [`examples/mods`](../../../examples/mods/README.md)。

`export_mod!` 通过 `lifecycle` 生成固定入口和生命周期回调，保留对所有 host lifetime
的实现约束，捕获 Rust panic 并在 C 边界返回失败。

该宏用于最终 DLL 入口。领域 provider 的内置适配实现 `mhf_mod_host::Module`，
包含本 SDK `Mod` 所没有的 `prepare_release`；不能把两者当成相同生命周期。
领域 crate 本身不调用 `export_mod!`，外部 DLL 必须在 detach 完成前归还自己持有的游戏 DLL 引用。

## 表示与验证

Host、Dependencies、接口绑定和 GameModule 都保留指针大小的表示；`Option<GameModule>`
使用空指针 niche。枚举、快照及透明包装没有新增堆分配，薄转发与 getter 使用 `#[inline]`。
配置字符串、错误信息和 Mod 实例分配仍按原有功能需要进行。

测试验证生命周期 panic／业务错误转换、UTF-8 配置与依赖错误、强类型日志和游戏阶段、
未知错误码往返，以及句柄大小。C／Rust 动态库示例检验生成函数表的实际调用。

头文件随使用方的 `build.rs` 自动生成到 `OUT_DIR/include`。构建依赖启用本 crate 的
`headers` feature 后，可使用 `abi::headers` 中的生成函数；游戏库、Counter 提供方和 Hook
示例均采用这条路径，生成期间复用各接口的 Rust 定义。

各 package 的 `tests/headers.rs` 默认检查构建产物与仓库快照。设置 `MHF_UPDATE_HEADERS=1`
显式更新快照，或设置 `MHF_HEADERS_EXPORT_DIR` 在检查通过后导出头文件。普通构建只写构建目录。
完整 Cargo 命令与 `safer-ffi 0.1.13` 的底层 feature 约束见
[头文件生成](../../../docs/dll-mods.md#头文件生成)。
