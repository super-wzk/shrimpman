# mhf-base

`mhf.base` 组合 Font、UI、Geometry、[Monster](../monster/README.md) 和 Quest。
各组件在自己的 crate 中实现，由 Base 统一安装和清理，不单独参与 Mod 选择与依赖解析。

游戏配置类型与 INI 映射属于 Base 的 [`config`](src/config.rs)。
`register_config(Config)` 向独立配置服务注册 `set`、`screen`、`video`、`sound`、
`localization`、`font`、`option`、`launch` 的默认值、字段类型和 INI 映射，并返回强类型
`MhfConfig`。`video.graphics_version` 通过通用 `fixed` 定义固定为 HD，保证游戏参数与
INI 读值一致；注册和读取不修改配置文件。

`BaseMod::new(settings, registry)` 接收 `MhfConfig` 和应用创建的共享 `OverlayRegistry`。Geometry 和 Monster 始终包含，Font 使用原生文本路径。
Base 创建 UI 输入状态，不提供游戏原生 IME 适配器，也不持有 Store 或 INI 桥。
配置服务通过 `mhf.config.v1` 提供能力，自身不依赖任何游戏配置类型。

Font、UI 和 Quest 都由 `mhf.base` 发布，接口名称分别为 `mhf.font.v1`、`mhf.ui.v1`、
`mhf.quest.v1`、`mhf.quest.control.v3` 和 `mhf.quest.launch.v1`。
消费者声明对 `mhf.base` 的依赖。应用将同一个注册表传给 Base、内置 Debug 与 Workbench；
外部 Mod 使用公开 UI 能力，不传递 Rust egui 对象。

Quest 初始没有本地会话。Debug 的启动回调通过 `QuestLaunch::prepare_local` 提交自己的预设
或自定义任务字节；Base attach 时仅为已选择的会话安装离线 Hook。普通 Login
启动不会因此安装离线网络或任务 Hook。任务字节保留原始日文编码。

生命周期顺序固定在 Base 内部：

- prepare 通过配置能力重复注册相同的 Base 定义，验证配置，准备并发布 Font/UI/Quest 接口。
- attach 依次安装 Font、Geometry、Monster、UI，再附加已选择的 Quest 会话。
- stop 关闭任务选择入口，再停止 UI 渲染与输入。
- detach 依次清理 Quest、Font、Geometry、Monster 和 UI。
- prepare_release 依次返还 Quest、Geometry、Monster 的额外游戏 DLL 引用，继续保留退役状态。

组件共用 `mhf.base` Context 和 Hook owner，INI 桥由独立配置 Mod 持有。
启动失败通过 stop/detach 回滚；清理失败时宿主保留 Base 并重试。
全部清理和引用返还成功后，宿主释放游戏 DLL，再销毁 Base 及其原生缓冲区。
