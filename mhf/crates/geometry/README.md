# MHF geometry

独立的 ZZ HD 模型几何扩展。FMOD 文件保持原格式，加载后的顶点索引、三角条带长度、
合并批次长度统一使用 32 位。原有模型无需重新导出。

启动器在 HD 游戏入口运行前安装，在游戏调用线程结束后卸载。在线启动和离线任务调试
共用同一入口；标准画质 `mhfo.dll` 不安装此模块。模块使用独立的 `mhf-hooks` 组，
不会启停其他功能的 hook，也不会修改磁盘上的 DLL。

## 数据链路

- `fmod.rs`：检查 MAIN/OBJECT、顶点数量和 32 位条带索引，保留绕序、材质映射和蒙皮变体。
- `mesh.rs`：生成 32 位索引缓冲内容、动态批次表和剔除包围球。条带连接保持原生的
  退化三角形与绕序，包围体按完整顶点索引计算。批次表不再依赖原生固定栈数组。
- `native/`：保留原生顶点和权重转换，替换其临时 16 位索引，并接管模型缓冲构造。
  GPU 创建沿用原生线程调度；分配、句柄登记、链表和 COM 所有权保持原生销毁协议。
  低着色器常量上限设备的 CPU 蒙皮数据布局也保持原样。
- `patches.rs`：四个原生绘制函数中的 94 处定点适配，覆盖长度读取、材质/变体偏移
  和批次指针步进。修改保持指令跨度不变，跳转地址保持有效。材质和骨骼标识仍遵循
  原生参数宽度；扩展的是几何索引与计数。
- `equipment_cache.rs`：武器与六个角色部件的同步、异步源文件缓存按实际文件长度增长，
  包含读取器追加的路径和 NUL，替代原有每槽 128 KiB 的静态空间。异步请求在执行时
  绑定缓存，保留排队与完成通知；同步读取继续经过原有的资源及本地化入口。
  33 处消费者引用随缓存迁移，旧分配保留到游戏线程停止，卸载时恢复原始引用。

支持 32 位索引的 D3D9 设备使用 `D3DFMT_INDEX32`。仅支持 16 位索引的设备仍可上传
原本的小模型，其 CPU 批次计数仍为 32 位；超过实际 `MaxVertexIndex` 或
`MaxPrimitiveCount` 的资源返回明确错误。整个进程仍是 i686，分配受到地址空间限制。

## 原生适配边界

已核对的映像为 `mhfo-hd.dll`，PE 时间戳 `0x5D6D7357`，`SizeOfImage = 0x0F11C000`，
静态分析基址 `0x10000000`。安装按实际加载基址计算 RVA，并在任何修改之前检查
入口和全部指令签名。不匹配的版本直接返回错误。

| 原生位置 | 模块处理 |
| --- | --- |
| `10002AF0` | 检查 FMOD，调用原生顶点转换，再用 32 位数据替换临时索引 |
| `10007B60` | 动态构建模型、批次、包围体和 D3D9 缓冲 |
| `10018D30`、`1001A1E0`、`1001B210`、`1001BC70` | 读取扩展批次表，保留原生材质与绘制逻辑 |
| `10007120`、`1158FFD0` | 通过核对过的寄存器适配器调用顶点转换和线程调度 |
| `108E22CB`、`108E2338`、`115904C0` | 防具/武器同步读取及异步请求执行前扩展源文件缓存 |

安装和卸载都要求游戏调用线程已停止。原生模型对象不保存指向 Rust 容器的指针，
由游戏原有的释放路径回收。

## 验证

在 `mhf/` 中运行纯数据测试；项目默认目标是 Windows，非 Windows 主机需要显式指定
本机 target，例如 Apple Silicon：

```sh
rtk cargo test -p mhf-geometry --target aarch64-apple-darwin
rtk cargo xwin clippy -p mhf-geometry --target i686-pc-windows-msvc --xwin-arch x86 --all-targets -- -D warnings
rtk cargo xwin build -p mhf-launcher --release --target i686-pc-windows-msvc --xwin-arch x86 --locked
```

测试覆盖旧模型绕序、65535/65536 顶点索引、70000 长度条带、合并超限批次、大量
材质批次、蒙皮包围体及损坏索引。真实 DLL 指令核对测试需设置
`MHF_GEOMETRY_TEST_CLIENT`，再运行 `instruction_edits_match_the_actual_client -- --ignored`。
Windows 上还可运行 `supported_client_installs_and_restores_geometry -- --ignored`，
核对实际 DLL 的安装、重复安装拒绝、卸载恢复和再次安装。

`tools/verify_native.py` 使用开发环境中的 `unicorn` 和 `capstone`，只读取指定 DLL，
在隔离的模拟内存中执行指令。它可以验证全部 94 处修改、真实原生静态/蒙皮转换器，
以及实际编译出的 10 个 x86 汇编适配器；这些 Python 包不是项目运行依赖：

```sh
rtk cargo xwin rustc -p mhf-geometry --lib --target i686-pc-windows-msvc --xwin-arch x86 -- -C codegen-units=1 --emit=obj
rtk proxy python tools/verify_native.py /path/to/mhfo-hd.dll \
  --object ../../target/i686-pc-windows-msvc/debug/deps/mhf_geometry-<hash>.o --converters
rtk proxy python tools/verify_equipment_cache.py /path/to/mhfo-hd.dll
```

两条 Python 命令在 `crates/geometry/` 中执行。这些验证不代替游戏内检查；实际高模的材质、
动画、阴影、不同画质及进出区域后的资源释放仍需在运行中的客户端验证。

装备缓存验证会复现旧缓存读取 130 KiB 文件时覆盖相邻内存，并执行真实加载器、
异步模式 2/7 和 33 处引用指令，覆盖 128 KiB 前后、130 KiB、1 MiB、尾部路径和
失败资源标记。文件/资源包 I/O 与系统事件使用模拟实现。
