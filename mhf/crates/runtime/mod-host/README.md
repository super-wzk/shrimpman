# mhf-mod-host

运行内置、原生 DLL 与数据 Mod。清单、版本选择和包导入由 [`mhf-mod-package`](../mod-package/README.md) 完成；
宿主消费已解析的 `Resolved`，不再次选择版本。

```rust,ignore
let mut mods = ModHost::load(resolved, &configuration, builtin_factory)?;
mods.prepare()?;                    // 加载游戏前资源与接口
let result = if unsafe { mods.launch(&mut launch_target)? } {
    game.load()?;
    mods.set_game(game.module_base());
    mods.check()?;                  // 所有目标的原像检查
    mods.attach()?;
    mods.running();
    Some(game.run())
} else {
    None                           // 取消启动，继续正常清理
};
mods.stop()?;                       // 消费者先于提供方，停止私有工作
unsafe { mods.detach(&builtin_order)?; }
unsafe { mods.prepare_release()?; } // 归还额外游戏 DLL 引用，保留退役缓冲
drop(game);                        // 最后的游戏 DLL 清理仍可访问上述缓冲
drop(mods);                        // 消费者实例先销毁，随后是提供方和各自 DLL
```

实际会话应在任何退出路径执行清理，并在失败时保留游戏引用及 Mod 宿主。
`retain()` 保留整个宿主。游戏仍可能运行时直接丢弃宿主，或 stop/detach/
prepare_release 失败后直接丢弃宿主，也会保留所有实例、C 上下文和 Mod DLL。

`Module` 是宿主内部 Rust 适配器，没有跨 DLL 传递。DLL 使用
`mhf-mod-api` 以 `safer_ffi::derive_ReprC` 定义并生成的 C 函数表。`create` 构造实例；接口和 Hook 只可在该
Mod 的 `prepare` 或 `attach` 回调中注册。回调成功后接口才对消费者可见，
失败时撤销本次未发布的接口。此前成功发布的接口一直保留到消费者销毁。

内置 Service 与 `Module` 实现放在各领域的 `provider` feature 中；
[`launcher/builtins.rs`](../../apps/launcher/src/builtins.rs) 只选择候选并调用各领域构造器。
内部 `Module` 的 `prepare_release` 用于归还游戏 DLL 引用并保留退役状态；
SDK 的 `Mod` 与 C 生命周期没有此阶段，领域 provider 不自动生成 DLL 入口。

启动由 prepare 发布的公共接口决定：普通 `mhf.launch.v1` 优先，没有普通提供方时使用
`mhf.launch.fallback.v1`，有效层必须只有一个提供方。`LaunchProvider::new` 创建普通入口，
`LaunchProvider::fallback` 创建默认入口；Login 使用 fallback，Debug 自动覆盖它。
回调返回 `Ok(false)` 表示取消，不视为失败。`GameInfoV2` 只报告阶段和可选游戏模块，基础入口为 `mhf_mod_query_v2`。

所有接口查询结果都是借用。功能 Provider 可通过 `derive_ReprC(dyn)` 和 `VirtualPtr`
发布生成的 vtable，宿主只登记表地址，不向消费者转移对象所有权。C 消费者不得复制
`VirtualPtr` 作为自有句柄或调用 `release_vptr`；提供方对象随宿主管理的实例销毁而释放。

Hook、窗口消息和私有线程的退出由对应适配器负责。外部回调执行期间不持有
接口注册表锁或 Hook 组借用。停止/拆除失败时跳过该 Mod 的提供方，继续处理
其他无关 Mod。成功的阶段不会重复执行；调用方可以明确重试失败的清理。

`builtin_order` 仅调整无依赖冲突的同级拆除顺序。所有依赖始终遵守消费者
先于提供方的顺序；Base 内部组件再按其固定次序停止与卸载。
依赖清理失败时保留提供方，后续 detach 与 prepare_release 继续遵守同一失败保留规则。

数据包自动发布 `mhf.data.v1`，`DataV1` 同样由 `derive_ReprC` 定义。生成的独立头文件
[`include/mhf_data.h`](include/mhf_data.h) 提供根目录与只读文件复制接口，
消费者通过普通 `dependencies` 和宿主 `dependency` 函数取得它，无需 DLL 或 SDK。

[`mhf-launcher/build.rs`](../../apps/launcher/build.rs) 在构建依赖中启用本 crate 的 `headers` feature，
随 Cargo 构建将 `mhf_data.h` 与基础、游戏头一起生成到 `OUT_DIR/include`。
在仓库根运行
`cargo test --manifest-path mhf/Cargo.toml -p mhf-launcher --target i686-pc-windows-msvc --test headers`
检查这三个构建产物与仓库快照。设置 `MHF_UPDATE_HEADERS=1` 显式更新快照；
设置 `MHF_HEADERS_EXPORT_DIR` 则在检查通过后导出头文件。完整约定见
[头文件生成](../../../docs/dll-mods.md#头文件生成)。
