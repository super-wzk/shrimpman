# mhf-mod-package

平台无关的 Mod 清单、发现、依赖解析与 ZIP 包读写。扫描和解析不加载 DLL。

```toml
schema = 1
id = "example.observer"
name = "战斗观察"
version = "1.2.0"
kind = "native"
entry = "observer.dll"

[dependencies]
"mhf.base" = "^1.0"
```

数据包使用 `kind = "data"`，不设 `entry`。包安装目录为
`<mods_dir>/<id>/<version>/mod.toml`，目录身份必须与清单一致。

宿主将 `discover(mods_dir)` 返回的目录候选与
`Candidate::builtin(manifest)` 构造的内置候选合并，再调用：

```rust,ignore
let selected = mhf_mod_package::resolve(
    &candidates,
    &selections, // BTreeMap<String, Selection>
    &defaults,   // 未明确禁用时启用的 ID
    &required,   // 应用固定要求的 ID
)?;
for candidate in selected.mods {
    // 每个提供方排在它的调用方之前；由宿主按 Source 加载实现。
}
```

`Selection` 的 `enabled: Option<bool>` 区分未配置和明确开关；
`version: Option<semver::VersionReq>` 限定所选版本。必要依赖自动补入，
但不会覆盖明确禁用。解析尝试最新兼容版本，遇到传递约束或循环时回溯。
同一 ID 只选择一个版本，相同 ID/版本的重复来源需要宿主先消除歧义。

具体应用提供候选清单、默认项和必需项。本 crate 不识别 Base、Debug 等功能，也不按候选来源附加依赖约束。
Launcher 的内建清单位于应用层 [`mhf-launcher-catalog`](../../apps/launcher-catalog/README.md)，由启动器与管理器共用。
`Source` 记录加载或打包位置，宿主据此调用应用工厂或加载 DLL；依赖解析统一使用清单里的 ID 和版本范围。

`export_archive(path, &selected.mods)` 导出传入的精确版本与全部包资源，
不会重新挑选版本。ZIP 内布局为 `mods/<id>/<version>/*`；`pack.toml`
记录所选 ID、版本与内置来源，内置二进制继续由宿主提供。

`Pack::read_archive(path)` 读取这份导出记录；
`import_archive(path, mods_dir)` 在暂存目录验证清单和相对路径后导入，
返回外部包候选，不覆盖已安装版本。它支持相同布局的单包 ZIP，
单包可省略 `pack.toml`。导入不会修改 `mhf.toml`，导出记录也不是第二份运行配置。

## 运行配置

`RuntimeConfig` 对应统一 `mhf.toml` 内的 `[mods]`；宿主与管理器共用该类型。
`RuntimeConfig::selections()` 提取启用状态与版本范围，`settings` 原样交给各 Mod。

```toml
[mods]
directory = "mods"

[mods."example.counter-consumer"]
enabled = true
version = "^1.0"

[mods."example.counter-consumer".settings]
label = "计数器"
```

`directory` 默认是 `mods`，相对路径的基准由调用方决定：游戏启动器使用其可执行文件目录，
并在切换游戏工作目录前解析；`mhf-mods` 使用命令启动时的当前工作目录。
`enabled` 未设置时由宿主默认选择或依赖关系决定；
明确禁用的必需依赖会使组合解析失败。状态和参数修改在下次游戏启动时生效。

可执行工具由 [`mhf-mod-manager`](../../apps/mod-manager/README.md) 提供，二进制名称为 `mhf-mods`。
不指定子命令时打开独立管理界面；包库本身不包含应用入口。
