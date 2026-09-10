# DLL Mod 组合示例

三个包使用同一个公开 C ABI。`example.counter` 提供计数器；Rust 消费者通过提供方的 `example-counter-sdk` 调用，C 消费者只包含 `mhf_mod.h` 和 `counter.h`。两个消费者在 `attach` 绑定已声明的依赖，分别增加 1 和 10，再输出当前快照。都启用时最终计数为 11；单个消费者日志中的数值取决于安装顺序。

Counter 只有 [`src/lib.rs`](counter-sdk/src/lib.rs) 中的一套公开定义：`Snapshot` 直接
`derive_ReprC`，`CounterApi` 生成虚表，`CounterTable` 直接是该 trait 的 `VirtualPtr`。
Rust `Counter::snapshot()` 按值返回同一个 `Snapshot`，`add()` 将溢出失败转换为 `Result`。
Provider 实现该 trait 并调用 `mhf_mod_sdk::host::register_interface` 发布借用表。
Rust consumer 使用 `LogLevel::Info`；C consumer 使用生成的 [`counter.h`](counter-sdk/counter.h)。

`cargo build` 通过 Provider 的 `build.rs` 自动生成 `OUT_DIR/include/mhf_mod.h` 和
`counter.h`，不会改写源码目录。`cargo test -p example-counter-provider --test headers`
检查它们是否与仓库中发布的头文件快照一致。接口修改后，显式设置
`MHF_UPDATE_HEADERS=1` 运行此测试更新快照；设置 `MHF_HEADERS_EXPORT_DIR` 则在检查通过后
将生成头文件导出到指定目录。详见 [头文件生成](../../docs/dll-mods.md#头文件生成)。

## Windows x86 构建

在 Windows 的 Visual Studio 开发环境中，从本目录执行：

```powershell
rustup target add i686-pc-windows-msvc
cargo build --release --target i686-pc-windows-msvc
$env:MHF_HEADERS_EXPORT_DIR = Join-Path $PWD "dist/include"
cargo test -p example-counter-provider --test headers --release --target i686-pc-windows-msvc --locked
Remove-Item Env:MHF_HEADERS_EXPORT_DIR
cmake -S counter-c -B counter-c/build -A Win32 -DMHF_SDK_INCLUDE_DIR="$PWD/dist/include"
cmake --build counter-c/build --config Release
./package.ps1
```

示例有独立 Cargo workspace；不修改宿主的锁文件。Rust 包输出分别为 `example_counter_provider.dll` 和 `example_counter_consumer.dll`。`package.ps1` 先运行头文件检查并导出到 `dist/include`，再将 DLL 重命名为各包清单指定的 `mod.dll`，复制清单和已验证的生成头文件到 `dist/mods/<id>/1.0.0/`；仓库根目录存在正式 `LICENSE` 时也会一并复制。已构建的 C DLL 和 [Hook DLL](hook/README.md) 也会打包。每个版本目录可单独压缩分发。

将 `dist/mods` 内的包放入启动器配置的 Mod 根目录，启用 `example.counter-consumer` 或 `example.counter-c`（或两者）。宿主会根据各自的 `[dependencies]` 选择 `example.counter`。不需要手动约定 DLL 加载顺序，也不需要把提供方实现链接进消费者。

## 接口和所有权

- `mod.toml` 是运行时包版本的唯一来源；Cargo 的版本描述源码 crate 的发布版本，不从 DLL 返回或与清单重复比对。
- `example.counter.v1` 对应 `CounterTable`；其 vtable 由 safer-ffi 从 `CounterApi` 生成，没有额外表外壳或快照镜像。
- 提供方以 `Rc<CounterTable>` 持有独立分配的表，通过 `Rc::as_ptr(&self.table)` 发布地址；消费者只借用表，调用时不克隆 `Rc`。宿主先销毁消费者，再释放提供方表、虚拟对象和 DLL。
- Host 返回的所有接口都只是借用。C 消费者保存 `const CounterTable *`，不得复制表取得所有权，也不得调用 `vtable.release_vptr`。
- `snapshot` 按值返回当前快照；`add` 在溢出时返回错误且不修改计数。两项操作支持并发，无输出指针保留或跨 DLL 分配释放。
- 基础 SDK 限定 Host 和依赖绑定的生命周期。具体接口、便利方法、线程和错误语义由 `counter-sdk` 的单一定义维护。
- 接口绑定位于 `mhf_mod_sdk::interface::bind`，业务调用仍是 `Counter::bind`、`snapshot` 和 `add`。
- 示例不安装游戏 Hook，可用作 DLL 发现、依赖解析和跨语言调用的最小验证。

公开宿主头文件位于 `../../crates/runtime/mod-api/include/mhf_mod.h`；纯 C 接入无需安装 Rust 或链接 SDK。CMake 未设置 `MHF_SDK_INCLUDE_DIR` 时使用仓库中的已发布快照，因此也可以单独构建 C 消费者。

取得借用表后，C 调用通过生成的对象和 vtable：

```c
const CounterTable *counter = (const CounterTable *)table;
CounterSnapshot snapshot = counter->vtable.snapshot(counter->ptr);
/* 使用 snapshot.count；不复制或释放 counter。 */
```

## 独立 ABI 验证

`abi_smoke.c` 是最小 C 宿主，加载三个实际动态库并验证查询入口、生命周期、两种语言的消费者调用和溢出错误。它不替代真实宿主的依赖解析测试。

Windows x86 开发者命令提示符中：

```bat
cl /W4 /I../../crates/runtime/mod-api/include /Icounter-sdk abi_smoke.c /Fe:target/abi-smoke.exe
target\abi-smoke.exe target\i686-pc-windows-msvc\release\example_counter_provider.dll target\i686-pc-windows-msvc\release\example_counter_consumer.dll counter-c\build\Release\mod.dll
```

也可以在 macOS 上验证同一 C ABI 的布局与直接调用（不验证 Windows 加载或游戏）：

```sh
cargo build
cc -dynamiclib -std=c11 -Wall -Wextra -Werror -I../../crates/runtime/mod-api/include -Icounter-sdk counter-c/mod.c -o target/counter-c.dylib
cc -std=c11 -Wall -Wextra -Werror -I../../crates/runtime/mod-api/include -Icounter-sdk abi_smoke.c -o target/abi-smoke
target/abi-smoke target/debug/libexample_counter_provider.dylib target/debug/libexample_counter_consumer.dylib target/counter-c.dylib
```
