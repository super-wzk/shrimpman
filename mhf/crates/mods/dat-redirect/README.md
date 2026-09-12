# DAT 文件读取重定向

`mhf.dat-redirect` 是默认关闭的独立内置 Mod，通过自身开关启用后，在正常登录、Debug 和 Workbench 下均生效。
它不依赖启动模式，Debug 也不依赖或自动启用它。
它将游戏目录 `dat` 下的文件读取映射到另一个根目录，
保留相对于 `dat` 的目录结构。替换文件缺失、不是文件或打开失败时，继续打开原文件。

在本次使用的 `mhf.toml` 中配置：

```toml
[mods."mhf.dat-redirect"]
enabled = true

[mods."mhf.dat-redirect".settings]
root = 'D:\mhf-overrides'
```

例如游戏读取 `D:\game\dat\em\model.bin`，优先打开 `D:\mhf-overrides\em\model.bin`。
`root` 默认为 `dat-redirect`，绝对路径直接使用，相对路径以游戏目录（`--game-dir`）为基准。
相对路径不能带盘符或根前缀；`C:overrides`、`\overrides` 需要改成完整绝对路径或普通相对路径。
根目录不需要再套一层 `dat`；允许根目录暂不存在，也允许替换文件对应的原文件不存在。
每次打开都会重新检查文件，已打开的句柄与游戏自身缓存不会自动刷新。

Mod 在 prepare 阶段安装 `CreateFileA`／`CreateFileW` Hook，覆盖游戏 DLL 初始化时的读取，
在 detach 阶段禁用并排空回调。仅重定向 `OPEN_EXISTING` 的只读文件打开，保留共享模式与其他打开参数；
写入、创建、删除、显式大小写敏感和 reparse point 打开保持原行为。目录枚举与属性查询不做重定向。

路径使用 Rust `Path`／`PathBuf`、`components`、`strip_prefix` 和 `join` 处理，解析绝对位置及 `..` 后
检查 `dat` 目录边界，不会误匹配 `database` 或逃出 `dat` 的路径。Windows 大小写兼容按原生路径组件比较，
ANSI 入口按当前文件 API 代码页转为 `OsString`，Unicode 入口保留 UTF-16，不用 UTF-8 有损转换。
映射基于请求的路径位置，不解析符号链接／目录联接的最终位置。
`\\?\` 路径保留原有含义；带 `.`／`..`、正斜杠，或无法在目标根目录保留的尾部空格／句点时，直接打开原路径。

Windows 测试：`cargo test -p mhf-dat-redirect --target i686-pc-windows-msvc`。
包含真实文件打开、ANSI／Unicode、缺失与共享冲突回退、写入隔离、Hook 卸载及重装。
