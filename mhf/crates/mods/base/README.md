# mhf-base

`mhf.base` 是一个内置运行时 Mod，组合 Font、UI、Geometry 和 Quest 服务。
各职责仍在原有 crate 中实现；它们的 `Module` 类型是 Base 的内部适配器，
不作为独立 Mod 参与选择和依赖解析。

游戏配置类型与 INI 映射属于 Base 的 [`config`](src/config.rs)。
`register_config(Config)` 向独立配置服务注册 `set`、`screen`、`video`、`sound`、
`localization`、`font`、`option`、`launch` 的默认值、字段类型和 INI 映射，并返回强类型
`MhfConfig`。`video.graphics_version` 通过通用 `fixed` 定义固定为 HD，保证游戏参数与
INI 读值一致；注册和读取不修改配置文件。

`BaseMod::new(settings)` 接收 `MhfConfig`。Geometry 始终包含，Font 使用原生文本路径。
Base 创建 `OverlayRegistry` 与 UI 输入状态，不提供游戏原生 IME 适配器，也不持有 Store 或 INI 桥。
配置服务通过 `mhf.config.v1` 提供能力，自身不依赖任何游戏配置类型。

Font、UI 和 Quest 都由 `mhf.base` 发布，接口名称分别为 `mhf.font.v1`、`mhf.ui.v1`、
`mhf.quest.v1`、`mhf.quest.control.v1` 和 `mhf.quest.launch.v1`。
消费者声明对 `mhf.base` 的依赖。`registry()` 供同一宿主二进制内的内置调试面板使用；
外部 Mod 使用公开 UI 能力，不传递 Rust egui 对象。

Quest 初始没有本地会话。Debug 的启动回调通过 `QuestLaunch::prepare_local` 提交自己的预设
或自定义任务字节；Base attach 时仅为已选择的会话安装离线 Hook。普通 Login
启动不会因此安装离线网络或任务 Hook。任务字节保留原始日文编码。

生命周期顺序固定在 Base 内部：

- prepare 通过配置能力重复注册相同的 Base 定义，验证配置，准备并发布 Font/UI/Quest 接口。
- attach 依次安装 Font、Geometry、UI，再附加已选择的 Quest 会话。
- stop 关闭任务选择入口，再停止 UI 渲染与输入。
- detach 依次清理 Quest、Font、Geometry 和 UI。
- prepare_release 依次返还 Quest、Geometry 的额外游戏 DLL 引用，继续保留退役状态。

Base 的组件使用同一个 `mhf.base` Context 和 Hook owner。INI 桥由独立配置 Mod 持有。
启动中途失败也通过上述
stop/detach 路径回滚；清理失败立即返回，宿主保留整个 Base 并重试。只有全部清理和
引用返还成功后，宿主才释放游戏 DLL，随后销毁 Base 及其原生缓冲区。
