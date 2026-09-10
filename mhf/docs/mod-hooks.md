# Mod 的 Hook

统一后端与 DLL C 接口具有 HD DLL 测试覆盖；游戏主循环中的裸汇编回调与交互退出仍需单独验收。相关契约见 [Mod 管理](mod-system.md) 和 [DLL Mod](dll-mods.md)。

## 职责与实现

| 单元 | 当前职责 |
| --- | --- |
| [`mhf-hooks::NativeGroup`](../crates/runtime/hooks/src/backend.rs) | 统一 MinHook 操作、目标占用、按组创建／启用／禁用／移除 |
| [`HookSet` / `HookGuard` / `HookSlot`](../crates/runtime/hooks/src/lib.rs) | 内置 Rust 状态发布、Invocation 回调计数与退役状态 |
| [`mod-host` C 适配](../crates/runtime/mod-host/src/context.rs) | 将组绑定到当前 Mod，保存私有排空回调，并在退出时清理 |
| [`mod-sdk::hooks::Hooks` / `HookGroup`](../crates/runtime/mod-sdk/src/hooks/mod.rs) | 包装公开 Hook 表；原始目标和回调状态的安全责任仍由 Mod 承担 |

MinHook 实现由宿主统一持有。内置组和经 Host C 表创建的 DLL 组进入同一个 `NativeGroup` 后端；DLL SDK 无需自行链接 MinHook。宿主基础能力无需写入 `dependencies`。

游戏目标、状态和内置 Module 适配位于各 `crates/mods/*` 的 provider 实现；
`runtime/game` 负责装配与最终会话释放，`runtime/hooks` 提供共享后端。
两个 launcher bin 只调用游戏宿主入口。

Mod 负责目标位置、原像、detour 调用约定和业务状态。管理 C API 使用 `extern "C"`，不改变目标可能具有的 `thiscall`、`stdcall` 或裸汇编约定。SDK 不将任意地址 Hook 声称为安全操作。

## 按组安装

公开 Rust 定义 [`HookApiV1`](../crates/runtime/mod-api/src/lib.rs) 使用 `safer_ffi::derive_ReprC`，生成 [`MhfHookApiV1`](../crates/runtime/mod-api/include/mhf_mod.h)：

| 操作 | 输入与结果 |
| --- | --- |
| `prepare_group` | 组名、私有状态指针、可选 `DrainFn` → 不透明组句柄 |
| `create_hook` | 组、目标、detour → trampoline 地址 |
| `enable_group` | 在 Mod 发布完状态后启用组内目标 |
| `discard_group` | 显式移除未启用组 |

准备、创建和启用只允许在 `prepare`／`attach` 阶段执行。Mod 必须先保存 trampoline 和全部回调状态，再启用；启用可能部分成功，因此错误回滚也需要禁用和排空。

Host、Hook 与示例的诊断表均从 Rust 定义生成。游戏库的 `build.rs` 生成包含 Host／Hook 表的
`mhf_mod.h`；Hook 示例的 [`build.rs`](../examples/mods/hook/build.rs) 复用
[`src/probe.rs`](../examples/mods/hook/src/probe.rs)，生成 `mhf_mod.h` 和 `probe.h` 到 `OUT_DIR/include`。
库和构建脚本使用同一份诊断表定义。各 package 的 `cargo test --test headers` 检查仓库快照，
更新与打包导出命令见 [头文件生成](dll-mods.md#头文件生成)。

```rust,ignore
// target、detour、state 和 drain 由具体 Mod 定义并验证。
let mut group = unsafe {
    mhf_mod_sdk::hooks::hooks(host).prepare_group("damage-observer", state, Some(drain))?
};
let trampoline = unsafe { group.create_hook(target, detour)? };
// 在此将 trampoline 保存进稳定的私有状态。
group.enable()?;
```

这是基础 API 用法示意，不表示仓库已经实现伤害观察接口。完整 detour 必须匹配真实目标的 ABI。字体、文本等现有内置实现继续复用其已知地址和目标检查，不增加通用特征扫描器。

组由宿主持有。丢弃 Rust `HookGroup` 包装本身不会卸载已启用 Hook；未启用组可调用 `discard`，其余由生命周期清理。DLL 自己提供排空机制，基础 SDK 不自动替任意 detour 管理回调计数。通过 Host 查询到的诊断或功能接口也只是借用；对含 `VirtualPtr` 的功能表，不得复制虚拟对象取得所有权或调用 `release_vptr`。

## 冲突规则

宿主进程内维护 `目标地址 → Mod ID、描述`。ModHost 在内置生命周期调用周围设置所有者；DLL C 表使用该实例的 Mod ID。

创建执行“占用目标 → MinHook 创建 → 保存组记录”。准备中的 Hook 也占用目标，同一 Mod 重复注册同样报错。冲突错误包含实际地址和双方描述，不按文件顺序、优先级或静默覆盖选择胜者。

创建失败且没有留下 Hook 时释放占用；创建成功后，只有成功移除才释放。禁用不释放占用。`ensure_released` 检查是否仍有该 Mod 的目标残留，失败时宿主保留实例和 DLL。

MinHook 记录目标入口；显式字节补丁使用 `PatchReservation` 记录完整区间，Geometry 的指令补丁已接入，成功恢复后才释放占用。区间重叠、已登记区间与 Hook 入口的冲突会报错。

**当前没有对外的字节补丁 C 接口，也未检测两个不同 MinHook 入口实际改写区间的重叠。** 后者需要后端给出真实机器码改写范围，不能用固定字节数估计。绕过宿主直接写内存或私自使用另一套 Hook 后端的 DLL，也不受这张表约束。

同一入口应由一个功能 Mod 拥有。需要共同观察时，由拥有者提供快照或明确的订阅接口；其他 Mod 通过 `dependencies` 和提供方 SDK 调用。多个修改者的合并语义由具体功能规定；首版没有任意 detour 链。

## 停止与释放

DLL 组的清理顺序为：

1. 所属 Mod 停止私有任务，宿主结束原生调用来源。
2. 禁用组内 Hook。
3. 调用 Mod 提供的 `drain(state)`，等待所有使用状态或 trampoline 的回调结束。
4. 移除 trampoline 并释放目标占用。
5. 按会话顺序销毁状态和 DLL。

排空回调使用 C ABI，返回 0 表示成功；非零则保留组、状态与 DLL。它不得提前释放其他原生代码仍可能访问的缓冲。无 SDK 的 C Mod 使用相同契约。

内置 `HookGuard::uninstall` 继续使用 `HookSlot`／`Invocation` 排空，并保留 `retired_state`。相关内置适配在最终游戏 DLL 卸载前归还额外引用，待 DllMain 返回后才销毁退役内存。

领域 provider 的内置 `mhf_mod_host::Module` 使用 `prepare_release` 完成上述引用归还；
DLL 的 `mhf_mod_sdk::Mod` 与 C 生命周期没有该阶段，必须在 detach 完成前结束自己的原生引用。
领域 crate 不自动定义 DLL 入口，也不会因启用 provider 就改变这两种生命周期契约。

UI 的 D3D9、窗口和 DirectInput Hook、Font Hook 与 Geometry 由 Base 的组件管理；
Quest 也归 Base，但只在 Debug 请求本地会话后安装任务 Hook。当前未接入 Unicode 原生 IME Hook。

detour 内不能安装或卸载 Hook。Invocation 覆盖状态读取和原函数调用；裸汇编中直接使用 trampoline 的入口，仍需先停止原生调用来源，不能只依赖 Rust 计数。

## 代码入口与验证

- [统一后端和目标占用](../crates/runtime/hooks/src/backend.rs)
- [组状态与回调排空](../crates/runtime/hooks/src/lib.rs)
- [C 组创建和清理](../crates/runtime/mod-host/src/context.rs)
- [字体 Hook 安装](../crates/mods/font/src/native/gdi.rs)
- [当前离线任务后端的原生入口](../crates/mods/quest/src/provider/native.rs)
- [游戏 DLL 最终释放](../crates/runtime/game/src/game/native.rs)

任务能力由 `mhf.base` 发布；`mhf.debug` 通过自己的 `quest` 设置选择任务文件，并提供临时猎人启动和调试工具。
任务字节与生成记录由 [`quest/binary.rs`](../crates/mods/quest/src/provider/binary.rs) 处理；其公开 `prepare_monster_spawn` 不安装怪物控制 Hook，也不直接操纵运行中的怪物。

[`example.hook`](../examples/mods/hook/README.md) 通过 Host C 表 Hook 自己 DLL 内的函数，并公开验证快照。[ModHost smoke](../crates/runtime/mod-host/examples/smoke.rs) 已有 Windows/Wine 覆盖：检查目标结果从 11 变为 111、退出后恢复为 11，进入／完成计数相等、活动数归零、drain 执行一次，并在同进程重复运行。

此外，[HD 会话测试](../crates/runtime/game/src/game/tests.rs) 检查最小启动提供方注入和游戏 DLL 加载／卸载；具体 Hook 由各组件测试覆盖。它调用 DllMain，不执行 `mhDLL_Main`；游戏运行中的迟到回调、窗口／IME 和实玩退出仍是交互验收项。首版不支持原生 Mod 热卸载。
