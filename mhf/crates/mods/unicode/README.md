# Unicode

本 crate 提供可独立使用的接口与可选 `provider`。启动器、Base、Quest 和管理器的运行清单均不包含该 provider；启用 Cargo feature 本身不会把组件注册到宿主。

默认层只提供 `decode_source`，可在创建离线 Session 时解码声明的源代码页，
不安装 Hook、不加载游戏 DLL。Windows 使用原有 Win32 解码规则；非 Windows
使用 `encoding_rs`，供 API 构建及离线数据准备使用。

`provider` 启用 i686 Windows 原生文本、资源、绘制和 IME 实现：

- `src/provider/module.rs`：`UnicodeMod` 生命周期与依赖接线。
- `src/provider/ime.rs`：游戏编辑器适配和原生 IME Hook。
- `src/provider/resources/`：按资源边界转码、查询公共 Translation API，并持有原生字符串 arena。
- `src/provider/native/`：经过签名验证的客户端文本入口。
- `src/provider/gdi.rs`：给 Font 组件注入的 UTF-8 GDI renderer。

## 宿主集成约束

`UnicodeMod::new` 接收是否启用翻译、IME 适配器共享槽和输入捕获共享槽。
自定义宿主需要显式组合该组件与 UI，并按生命周期顺序连接共享状态：check 创建
IME 适配器，UI attach 安装渲染与输入并发布 capture，Unicode attach 再安装
原生 IME 和文本 Hook。Base 不提供 `unicode` feature。

stop 先停原生 IME，detach 停文本和资源入口；prepare_release 只返还游戏 DLL 引用。
arena、字形和适配器状态必须保留到宿主完成原生卸载，防止游戏持有悬空指针。

## 资源布局

资源结构的唯一来源是 [共享资源布局](../../shared/resource/resources/layout.json) 和
原生 [layout.rs](../../shared/resource/resources/native/layout.rs)。Unicode build.rs 生成
`OUT_DIR/resources.rs`；Translation 的构建脚本复用同一个 catalog 解析源，独立生成
词典，不依赖 Unicode Cargo provider。运行时跨领域仅传稳定字符串 Key，词典 ordinal
不进入 Unicode 资源结构。
