# mhf-mod-manager

独立 Mod 管理应用，二进制名称为 `mhf-mods`，不指定子命令时打开图形界面。
它使用 [`mhf-mod-package`](../../runtime/mod-package/README.md) 发现、解析和读写包，
并编辑同一份 `mhf.toml` 的启用设置。管理器与游戏宿主共用内置元数据和 semver 依赖解析器，
检查配置中明确启用的 Mod 及其声明依赖。自动默认项、启动必需项和最终加载顺序由启动器决定。
管理器不加载 Mod DLL 或游戏。

## 图形界面

在要管理的运行目录执行 `mhf-mods`。界面支持：

- 刷新并查看内置 Mod、已安装外部包及其声明依赖。
- 设置「自动／启用／关闭」，输入 semver 范围或选择已安装的精确版本；「自动」保留启动器的选择规则。
- 预览依赖解析结果与冲突，保存设置或撤销修改。保存保留原 TOML 的其他字段与注释，下次启动游戏生效。
- 在后台导入、导出 ZIP。导入不自动启用，也不覆盖已有包版本；导出已保存配置中明确启用的项及其完整依赖，包含所需外部包和内置 Mod 的精确版本记录，输出文件必须尚不存在。

配置和 Mod 目录在启动时确定，需要覆盖时使用 `--config`、`--mods-dir`。
ZIP 路径可在导入／导出对话框中输入，Windows 还提供原生文件选择器。

```sh
mhf-mods
mhf-mods --config ./mhf.toml --mods-dir ./packages
```

## 构建与开发命令

工具本身可在宿主平台运行。构建 Windows 版本时，在仓库根运行：

```sh
cargo build --manifest-path mhf/Cargo.toml -p mhf-mod-manager --release --target i686-pc-windows-msvc
```

Cargo 默认启用 `gui`、`login`、`debug`。
`login`、`debug` 决定管理器可列出的启动提供方，手动裁剪时与所管理的游戏构建对应。
关闭 `gui` feature 后仍可使用命令行子命令。

在项目 Nix 开发环境中，`mhf-mods-build` 构建 Windows i686 release 管理器，
`mhf-mods` 构建后打开图形界面；也可使用 `nix run --impure .#mhf-mods`。
Nix 使用 `gui,login${debugFeature}`，与项目启动器共用调试能力设置。
包装命令在源码 `mhf/` 目录的子进程中构建，运行工具时保留调用者的当前工作目录，
使用现有 Wine／WSL 设置。与 Nix 启动器共用配置优先级：显式 `--config`、`MHF_CONFIG`、当前工作目录的 `mhf.toml`。
配置不存在就报错，不生成副本或回退到其他目录。
需要另选配置时，显式设置 `MHF_CONFIG`，包装命令会将其传给 `--config`，例如
`MHF_CONFIG=/path/mhf.toml mhf-mods list`。其中的相对路径同样按调用目录解析。
运行管理器不需要设置 `development.mhf.gameDirectory`。

## 命令行

显式传入子命令时继续使用 CLI，例如 `mhf-mods list` 或 `nix run --impure .#mhf-mods -- list`：

```sh
mhf-mods list
mhf-mods import counter-pack.zip
mhf-mods enable example.counter-consumer --version "^1.0"
mhf-mods disable example.counter-consumer
mhf-mods export selected.zip
mhf-mods export counter.zip "example.counter-consumer@=1.0.0"
mhf-mods --config ./mhf.toml --mods-dir ./packages list
```

- `list` 显示当前构建可用的内置 Mod、外部包版本、类型、配置开关、版本范围和来源。配置中存在但没有可用包的 ID 也会列出。
- `import <ZIP>` 导入包，保留启用设置；已有版本不会被覆盖。
- `enable <ID> [--version <范围>]`、`disable <ID> [--version <范围>]` 只修改同一个 `mhf.toml` 内的指定 Mod 条目，保留其他配置、参数与注释。不传版本时保留已有要求。
- `export <ZIP> [ID[@范围] ...]` 先解析已安装包的依赖，导出精确版本及完整依赖。省略 ID 时以配置中明确启用的 Mod 为根；显式 ID 列表替换本次导出的根选择，保留依赖的配置版本约束和明确关闭状态。命令行选择不写回配置。输出文件必须尚不存在。

GUI 导出与不指定 ID 的 CLI 导出使用相同的根选择。需要包含宿主默认项的完整运行组合时，
使用启动器的 `--export-modpack`。

## 路径规则

所有相对路径都以命令启动时的当前工作目录为基准：默认配置为 `./mhf.toml`，
缺省 Mod 目录为 `./mods`，配置中的相对 `[mods].directory` 也从该目录解析。
配置文件必须已存在；读取失败会报错。
显式 `--config`、`--mods-dir` 以及 ZIP 路径使用相同规则，绝对路径直接使用；
`--mods-dir` 只覆盖本次操作。即使 `--config` 指向其他目录，配置中的相对 Mod 目录
仍以命令启动时的当前目录为基准。工具自身放在哪里不影响这些路径。
