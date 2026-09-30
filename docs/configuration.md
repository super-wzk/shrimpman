# 配置说明

[返回项目入口](../README.md) · [开发环境](development.md) · [验证](validation.md)

## 本地覆盖

`local/` 整个目录由 Git 忽略。创建 `local/default.nix`：

```nix
{ pkgs, mkCommand, ... }: {
  development.stateDirectory = ".state";
  mhf.screen.window_resolution = { width = 1280; height = 720; };
  mhf.sign.endpoint = "http://127.0.0.1:53011";
  development.mhf = {
    gameDirectory = "/path/to/mhf";
    runner = "wine";
  };

  development.packages = [ pkgs.ripgrep ];
  development.commands.hello = mkCommand {
    name = "hello";
    text = ''
      echo "Hello from the local module"
    '';
  };
}
```

本地文件是完整 Nix 模块，可导入其他模块，设置 `development.environment`、
`development.shellHook` 及 Process Compose 的 `settings`、`defaults`、`cli`。
共享默认值使用 `lib.mkDefault`；需要替换同优先级选项时使用 `lib.mkForce`，列表遵循 Nix 模块合并规则。

Git flake 不包含忽略文件，因此开发模块从 `PROJECT_ROOT`（未设置时为 `PWD`）读取工作树中的
`local/default.nix`，需要 impure 求值。从仓库根运行：

```sh
nix develop --impure
nix run --impure .#mhf-launcher -- --config mhf/mhf.toml
```

纯 `nix develop` / `nix run` 使用共享模块；本地模块不参与 `flake.lock`。
已有环境变量优先于 `development.environment` 的默认值。
Nix 模块会进入 Nix store，凭据应通过运行时环境提供。

## 状态目录

`development.stateDirectory` 默认 `.state`，接受绝对路径或相对于仓库根的路径。
初始化过程在运行时导出绝对的 `PROJECT_ROOT` 和 `PROJECT_STATE`，避免引用 Nix store 中的源码副本。
`PROJECT_STATE` 是解析结果，修改目录应设置 `development.stateDirectory` 并重新加载 shell。
Wine 默认前缀位于 `$PROJECT_STATE/wine`；direnv 缓存位于 `.direnv/`。
客户端 Process Compose 控制 API 默认端口为 `8081`，可通过 `cli.options.port` 覆盖。
服务端进程组默认使用 `8080`，两组进程可同时运行。

## 生成配置

[`mhf/config.nix`](../mhf/config.nix) 是公开 TOML 默认值的来源；可在本地模块中覆盖 `mhf` 嵌套属性。
更改公开默认值后，从仓库根运行：

```sh
nix run .#update-configs
nix flake check
```

生成目标是 `mhf/mhf.toml`。
`update-configs` 即使带 `--impure` 也只输出共享默认值；审查并随 Nix 模块一起提交。
`nix flake check` 检查生成结果是否与提交的文件一致。

## 客户端路径与传输

Nix 启动器和 Mod 管理器命令的配置选择顺序为：显式 `--config`、`MHF_CONFIG`、调用目录的 `mhf.toml`。
直接运行 EXE 时使用 `--config`，未指定则读取调用目录的 `mhf.toml`；EXE 不读取 `MHF_CONFIG`。
配置文件必须存在，工具直接读写该文件。需要个人副本时先复制提交的默认文件。
`MHF_CONFIG` 和 Mod 配置中的相对目录以调用目录为基准；指定另一个目录中的配置文件不会改变基准。

`development.mhf.gameDirectory` 接受绝对路径或相对于仓库根的路径。
`development.mhf.runner` 为 `null` 时自动检测 WSL 互操作，否则使用 `wine`；
空字符串表示直接执行 EXE，也可指定 Wine 程序。PATH 之外的运行器建议使用绝对路径。
已有 `WINEPREFIX` 优先于状态目录默认值。
WSL 直接执行时转换配置、游戏路径，并通过 `WSLENV` 转发 `MHF_*` 变量，保留已有转发规则。
进入 shell 或仅构建工作区不会创建 MHF 运行文件。

`mhf.sign.endpoint` 默认是 `http://127.0.0.1:53001`，URI scheme 选择 HTTP、HTTPS 或 TCP。
例如在本地 Nix 模块中设置 `mhf.sign.endpoint = "tcp://127.0.0.1:53000";`，也可用
`MHF_SIGN__ENDPOINT=tcp://127.0.0.1:53000` 覆盖整个 URI。
客户端与服务端的端口分别配置；使用自定义服务端口时，客户端需要设置对应的完整 URI。
Sign TCP 文本编码默认 `utf8`；连接使用 Shift-JIS 的 Erupe 时设置
`mhf.sign.encoding = "shift_jis";` 或 `MHF_SIGN__ENCODING=shift_jis`。
此编码选项仅作用于 Sign TCP；应用使用原生游戏文本。

## 启动功能

Nix 的 `development.mhf.debug.enable` 和 `development.mhf.workbench.enable` 默认均为 `true`，
决定构建是否包含对应实现；运行时仍由 `[mods]` 选择。
例如在选定的 `mhf.toml` 中启用 Debug：

```toml
[mods."mhf.debug"]
enabled = true

# 可选：省略时使用内嵌任务。
[mods."mhf.debug".settings]
quest = "quests/test.bin"
```

Debug 的普通启动接口优先于 Login 的 fallback。Debug 与 Workbench 同时启用会造成普通启动提供方冲突。
Quest 由 Base 管理，在启动提供方请求本地会话后安装任务 Hook。
功能选项见[启动器](../mhf/crates/apps/launcher/README.md)和 [Mod 系统](../mhf/docs/mod-system.md)。
