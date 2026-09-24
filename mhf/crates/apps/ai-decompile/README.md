# mhf-ai-decompile

离线反编译指定物种、地图的怪物 AI 工程。不加载 DLL、不执行游戏代码，不需要启动游戏或连接 IDA。

```sh
# 当前目录为游戏目录，物种 ID 和地图 ID 均必填
mhf-ai-decompile 1 31

# 从其他目录运行：环境变量覆盖输入游戏目录
MHF_GAME_DIR="/path/to/game" mhf-ai-decompile 1 31
```

PowerShell：

```powershell
$env:MHF_GAME_DIR = 'D:\Games\mhfz'
.\mhf-ai-decompile.exe 1 31
```

使用 `--help` 查看帮助。物种 ID 和地图 ID 均必填。
可用 `--game-dir` / `-d` 指定游戏目录，优先于 `MHF_GAME_DIR`，未指定则使用当前目录。
读取 `<游戏目录>/mhfo-hd.dll`。相对路径按当前工作目录解析，空值报错。
`--config` / `-c` 不参与离线导出，其指定的文件无需存在。

输出固定在**当前工作目录**，不受 `MHF_GAME_DIR` 影响：

```text
ai-export/
  monster-ai/
    maps/31/1/
      main.mhai
      export.txt
```

不同地图、物种的工程可共存；同一地图和物种的目录存在时默认拒绝覆盖。
使用 `--overwrite` 覆盖该工程的生成文件（含 `export.txt`），保留其他文件及相邻工程：

```sh
mhf-ai-decompile 1 31 --overwrite
```
可将导出的 `monster-ai` 目录复制到游戏 `dat/` 下使用。工具不直接写入游戏目录，也不自动启用覆盖。
导出前会重新编译工程，检查生成的 DSL 是否有效。`export.txt` 记录 DLL SHA-256、入口地址、物种、地图和提取范围。

## 范围与限制

- 输入 DLL 必须使用 ZZ HD 固定地址布局，地图索引范围为 `0..97`。
  工具不自动识别布局；不兼容的 DLL 可能导致读取失败或错误解释。
- 物种 166 的入口还依赖任务/运行时状态，只有物种和地图不足以确定，因此拒绝导出。
  原生返回空入口的情况单独报错。
- PE 地址读取只允许磁盘实际存在的数据。BSS、运行时指针、越界和截断读取不会被补零。
  任何反编译警告均中止导出，不静默生成缺失内容的工程。
- 从状态 0、事件入口和显式子脚本引用递归恢复。
  动态选择、未被这些引用发现的脚本，以及原生动作函数不属于提取范围。
  **输出为 `base native` 工程，依赖游戏原生 AI。**

## 构建和验证

Nix 命令构建 Windows EXE，通过 Wine／WSL 或 `development.mhf.runner` 指定的程序执行：

```sh
mhf-ai-decompile-build
mhf-ai-decompile 1 31
# 使用本地 Nix 游戏目录配置；自动构建后执行
nix run --impure .#mhf-ai-decompile -- 1 31
```

Nix 命令保留调用目录，传入 `--config` 和 `--game-dir`。
需设置 `development.mhf.gameDirectory`；配置中的相对路径按 `PROJECT_ROOT` 解析。
由于包装传入显式 `--game-dir`，Nix 配置在该入口优先于 `MHF_GAME_DIR`。
直接运行 EXE 时，游戏目录按 `--game-dir` → `MHF_GAME_DIR` → 当前目录的顺序选择。
`mhf-ai-decompile-build` 使用 `i686-pc-windows-msvc` target。

在 `mhf/` 下执行（仓库默认目标为 Windows i686）：

```sh
cargo build -p mhf-ai-decompile --release
cargo test -p mhf-ai-decompile --target aarch64-apple-darwin
MHF_AI_DLL="/path/to/game/mhfo-hd.dll" \
  cargo test -p mhf-ai-decompile --target aarch64-apple-darwin real_dll_projects_compile -- --ignored
```

其他主机需替换测试目标 triple。真实 DLL 测试不修改游戏文件。
