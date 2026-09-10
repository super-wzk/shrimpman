# Quest

本 crate 提供任务数据、控制接口和原生组件，由运行 Mod `mhf.base` 持有。
它不再作为独立包选择。公开接口是 `mhf.quest.v1`、`mhf.quest.control.v2`、`mhf.quest.launch.v1`，
三者提供方 ID 均为 `mhf.base`。

默认层提供 API 与 Rust 绑定；`provider` 增加 Session、任务解析和原生实现。
`Session::new` 读取调用方提供的 BIN/JKR，保留原始文本字节。Quest 不包含预设任务，也不接受空数据作为默认任务。

Base 的 prepare 发布任务快照、控制和本地启动接口。此时任务 Hook 尚未安装。
Debug 的启动回调调用 `mhf.quest.launch.v1` 的 `prepare_local`，传入 Debug 自己的预设任务或自定义文件字节；
Base 随后在 attach 安装本地任务 Hook。普通 Login 不调用这条接口，因此保持任务组件闲置。

自定义任务由 Debug 的设置指定：

```toml
[mods."mhf.debug".settings]
quest = "quests/test.bin"
```

相对路径以 Debug 的资源目录为基准，内置实现对应启动器可执行文件目录；绝对路径直接使用。
省略 `quest` 时由 Debug 提供其预设任务，Quest 只负责生命周期。启动方法见 [Debug](../debug/README.md)。

服务、会话和已发布表保留到 Base 最终销毁。detach 卸载任务 Hook，prepare_release 归还额外游戏 DLL 引用；
宿主卸载游戏后才销毁退役缓冲。快照和任务字节处理可独立验证，原生操作要求受支持的客户端。

`MonsterSpawn.variant` 接受原生编号 0 到 16：普通（0）、HC（1）、辿异种（16），其余编号按物种解释。
调用方负责确认物种支持该变种，例如极吼雷狼龙使用雷狼龙物种与变种 11。
Quest 按任务怪物资源槽写入变种，保留其他物种、奖励模式和全局任务标志。
普通任务前两个资源槽支持变种，原任务启用 Interception 时扩展到前五个；
强化变种落在其余槽位会报错，普通变种仍可使用原有的六个资源槽。
控制接口升级为 `mhf.quest.control.v2`，旧接口结构中的填充字节不能作为变种值读取。

在 `mhf/` 目录可运行 `cargo test -p mhf-quest --features provider` 验证任务解析和预处理。
