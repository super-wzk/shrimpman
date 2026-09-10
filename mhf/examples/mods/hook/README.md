# Host Hook 示例

`example.hook` 只修改自己 DLL 内的 `noinline` 函数。`attach` 使用宿主的 C Hook API 创建一组 Hook，保存 trampoline 后启用，并验证 `invoke(10)` 从 11 变为 111。没有独立 MinHook 实例。

原始操作通过 `mhf_mod_sdk::hooks::hooks(host)` 和 `host::register_interface` 接入。
普通 Rust Host 不暴露 detour、drain 或裸指针方法；该示例实现原生 Hook，直接使用所属模块的原生操作。

每个 detour 在使用状态和 trampoline 的整个期间计入 `active`。宿主在 `stop` 后禁用 Hook，再调用 `drain` 等待计数归零，最后删除 trampoline。示例只允许生命周期线程调用 probe，`stop` 到 `detach` 完成期间不发起新调用。状态和公开函数表一直保留到 Mod 销毁。

公开诊断表使用 `Rc` 保持稳定共享地址，回调状态使用 `Arc` 和内部同步；生命周期的可变 Mod 借用不会取得这些已发布数据的独占访问。

诊断结构使用 `safer_ffi::derive_ReprC`，生成的 [`probe.h`](probe.h) 供真实宿主烟测在清理后验证：函数恢复为 11，进入与完成计数相等，没有活动调用，且 drain 恰好执行一次。Host 返回的诊断表也是借用，不转移实例或状态所有权。

诊断类型和标识符只定义在 [`src/probe.rs`](src/probe.rs)，DLL 和 `build.rs` 共用。
`cargo build` 自动生成 `OUT_DIR/include/mhf_mod.h` 和 `probe.h`，不修改源码目录。
`cargo test --test headers` 检查生成结果与发布快照是否一致；接口修改后，显式设置
`MHF_UPDATE_HEADERS=1` 运行此测试更新快照。设置 `MHF_HEADERS_EXPORT_DIR` 则在检查通过后
将生成头文件导出到指定目录。完整说明见 [头文件生成](../../../docs/dll-mods.md#头文件生成)。

在本目录的 Windows x86 开发环境中，先按上级目录说明构建并打包 Counter 的三个示例，再执行：

```powershell
cargo build --release --target i686-pc-windows-msvc
../package.ps1
cargo run --manifest-path ../../../Cargo.toml -p mhf-mod-host --example smoke --target i686-pc-windows-msvc -- ../dist/mods
```

烟测使用实际 `ModHost` 完成发现、SemVer 依赖解析和 DLL 加载，确认 Rust/C 消费者使 Counter 等于 11，再检查 Hook 清理。整个流程在同一进程重复两次，验证资源可以释放后重新加载；无需启动游戏。
