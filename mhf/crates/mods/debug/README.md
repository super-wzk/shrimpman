# Debug

`mhf-debug` 提供运行 Mod `mhf.debug`，仅依赖 `mhf.base`。
它在 prepare 发布普通启动接口 `mhf.launch.v1`，自动覆盖 Login 的 fallback，
回调先通过 Base 的 Quest 启动接口选择本地任务，再填充临时猎人固定字段；
游戏内工具继续由 `DebugToolsMod` 内部组件实现，共用同一个 Mod 所有权与生命周期。

只有一个 `mhf-launcher`。在它读取的 `mhf.toml` 中启用 Debug：

```toml
[mods."mhf.debug"]
enabled = true

# 可选；省略时使用内置古迹任务。
[mods."mhf.debug".settings]
quest = "quests/test.bin"
```

然后执行 `mhf-launcher`。任务的相对文件路径基于 Debug 的资源目录（内置实现对应启动器可执行文件目录），
绝对路径直接使用。Wine 下可以使用对应的 Windows 路径；游戏目录和配置文件仍通过 `--game-dir`、`--config` 指定。
关闭 `mhf.debug` 后，默认 Login 恢复接管启动，无需维护另一份模式配置。

默认任务使用极驱迅龙的古迹大地图，从营地 460 出生。任务文件编译进应用，不需下载。
任务文字保留原始日文编码。
指定文件可为原始 BIN 或 JKR 类型 3 压缩文件，接受编号 40000 以上的活动任务，解压后不得超过 32 KiB。

`DebugModule::new(OverlayRegistry) -> Self` 接收内置 Base 的共享界面注册表；
prepare 解析自身设置并绑定 `mhf.quest.launch.v1`，实际任务文件在启动回调执行时读取。
未指定文件时读取 Debug 自己的 [`test-map.bin`](resources/quests/test-map.bin) 预设；始终向 Quest 提交实际字节，空数据或非法任务会使启动失败。
check、attach、stop、detach、prepare_release 转发调试组件，完整 egui 调试窗口要求内置 Base。
对外调试接口仍为 `mhf.debug-tools.v1`，其提供方 ID 为 `mhf.debug`。

游戏内按 F7 显示或隐藏窗口。装备、招式、换区、变身和任务重开见
[调试工具操作](../debug-tools/README.md#调试操作)，任务数据与控制接口见 [Quest](../quest/README.md)。
