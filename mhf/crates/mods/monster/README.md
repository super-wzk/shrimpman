# Monster

ZZ HD 客户端的怪物领域 crate：把 `dat/monster-ai` 里的 `.mhai` 编译成图，叠到
actor 当前的原生 AI 块上，加上种类上限补丁。由 [`mhf.base`](../base/README.md)
管理生命周期；它是 Base 内部组件，`provider` 仅在 i686 Windows 上启用。

固定构建是 `mhfo-hd.dll`（镜像基址 `0x10000000`，SHA-256
`95c580195f4080d2e9582c8c9df36abeb280476e088b6366583c5f138da8f301`）：补丁地址、
Hook 签名和文档里的偏移都只对这个构建成立。

## 模块

`src/ai/` 是怪物 AI 功能，`src/species/` 是种类上限功能，两者互不相识；`src/native.rs`
是它们共用的游戏内存原语与构建指纹；`src/lib.rs` 只做聚合，由 `MonsterMod` 决定两者
attach、回滚和 detach 的顺序。功能各自的代码不放在源码根目录。

| 位置 | 内容 |
| --- | --- |
| `lib.rs` | 顶层聚合：`MonsterMod` 的补丁／Hook 生命周期与回滚 |
| `native.rs` | 共享原语：游戏内存的定长读写与已验证构建的 PE 指纹 |
| `ai/mod.rs` | `Program`／`Node`／`Table`／`Base` 与 `validate_lossless` |
| `ai/dsl/` | 多文件工程：词法、语法、命名空间解析及函数展开；`Project::load` → `Project::compile` → `Program` |
| `ai/bind.rs` | 把 `base native;` 声明叠到活块上：读窗口、写私有 descriptor／状态表／事件格与脚本 |
| `ai/bytecode.rs` | 已命名 opcode、选择子宽度与 `is_stop` |
| `ai/decompile.rs` | 有界读取可追踪状态与事件入口，生成函数式 DSL 并校验字节往返；这是部分提取，未知语义使用 `native(...)` |
| `ai/control.rs` | 编译器、绑定和覆盖共用的常数：事件槽、主表下标、路由掩码 |
| `ai/overlay.rs` | `dat/monster-ai` 的 Hook 与加载：签名校验、`(map, species)` 会话缓存、私有块发布 |
| `species/mod.rs` | 8 处上限的预留、写入、还原与 DLL 引用 |
| `species/patches.rs` | 8 处上限的 RVA 与原始／替换字节 |
| `docs/` | [`dsl-spec.md`](docs/dsl-spec.md) 语言规范、[`opcode-catalog.md`](docs/opcode-catalog.md) 等逐 opcode 目录、[`runtime-fields.md`](docs/runtime-fields.md) 实体字段词典 |

事件检查顺序是 `0x40/root[14]`、`0x80/root[13]`、`0x20/root[4]`、
`0x10/root[3]`、`0x08/root[11]`、`0x04/root[10]`、`0x02/root[8]`；高 lane
互斥，低 lane 可能先更新若干保存游标。`0x05` 是动作请求，`0x07` 切换主表条目；
137 个已命名 opcode 和其余字节的默认终止行为记在 opcode 目录里。作者使用固定
事件名及状态别名／索引；事件槽位、mask 和收尾指令由编译器映射；规范里尚未定论的
部分（`repeat`、`resume()`、`self.`／`if`、框架内部命令 `reset`）
编译器直接拒绝，`native(...)` 是显式逃生口并报告它代表的字节。

## 种类上限补丁

补丁将 8 处比较上限从 `177` 改为 `255`，允许 `177–254` 通过；`0–176` 的结果不变，
`255` 仍执行原有回退分支。指令长度、无符号跳转和字段位宽保持原样。

| 路径 | 补丁 RVA | 原有越界处理 |
| --- | --- | --- |
| 动作分派 | `0x0086A662`、`0x0086E95B` | 将 actor species 清为 0 |
| 初始化 | `0x0086E494` | 将 actor species 清为 0 |
| 每帧行为分派 | `0x0086E8C5` | 将 actor species 清为 0 |
| 模型参数 | `0x008FD3A3` | 该次查询使用种类 1 |
| 创建时基础属性 | `0x00AAA45A` | 该次查询使用种类 1 |
| 音效参数 | `0x00B4698C`、`0x00B47849` | 该次查询使用种类 1 |

新增种类的数据表、行为和资源需由使用者另行提供。原始客户端的 `0x118C3350`、
`0x118C3628`、`0x118C38F0` 回调表各只有 177 项；`0x118C3628` 表中索引 177 已是
相邻数据 `0x2FF`，不能作为回调调用。初始化还会索引其他静态参数表。
仅应用这 8 处补丁就生成新 ID 会越界；其他种类检查、调试目录和 Quest 校验仍保留原限制。

attach 在游戏入口前校验 PE 版本和原始指令，保留 DLL 引用、预留补丁范围并写入代码。
detach 逆序还原指令和内存保护；失败时保留状态供宿主重试。
prepare_release 仅在全部补丁还原后返还 DLL 引用。

## 怪物 AI 覆盖

crate 还在记录初始化 `0x00860360` 上安装 Hook（安装前校验共享的镜像指纹与签名
`8a 56 03 a1 3c ff 7f 1e`）。该函数是客户端里唯一写 actor `+9F0`（物种块指针）
和 `+9F4`/`+A5C`（状态游标）的路径；Hook 在它返回后把 `+9F0` 换成
`dat/monster-ai/maps/<map>/<species>/main.mhai` 生成的私有块，并按私有状态表和 actor
`+A10` 重算游标。原生初始化先跑完，覆盖后生效。

每个 `(map, species)` 在首次取用时读取工程，地图入口不存在时回退到
`dat/monster-ai/common/<species>/main.mhai`（见 [DSL 规范](docs/dsl-spec.md)），解析、编译，
再在活着的物种块上叠出私有块；没有 `base native;` 的文件被拒绝。文件不存在
时该格保持原生并记住"没有文件"；文件存在但解析或绑定失败是硬错误，该 actor
保持原生、每次生成都记录并重试，改好文件后不必重开会话。species 大于 `0x83`
的 actor 不走这张表，直接跳过。

AI Hook 与上限补丁共用生命周期：attach 先写补丁再装 Hook，Hook 安装失败时
回滚补丁；detach 反序卸载。

入口通过 `states { idle -> idle_loop; }` 和 `events { player_detected -> reactions.handle; }`
绑定函数；省略索引时顺序分配，`= N` 指定下一起点。函数写作 `fn idle_loop()`，
使用 `combat.attack();` 调用，自然结束自动返回；事件按槽生成原生收尾。
主状态仍需显式切换或终止。`import "#common/combat.mhai" as combat;` 只访问
当前物种默认目录；普通路径相对于当前文件。函数在编译期展开，不覆盖原生子脚本表。
完整可编译示例位于 [examples/monster-ai](examples/monster-ai/)。

## 验证

语言、编译器和绑定层的测试不依赖 Windows：

```sh
cargo test --manifest-path mhf/Cargo.toml -p mhf-monster --target aarch64-apple-darwin
```

`provider` 的编译与链接属于 i686 目标，按工作区 README 的命令检查。上面这些
仍是静态与宿主测试的结论，没有游戏实机验证。

验证脚本读取真实 DLL，在 Unicorn 内存中执行原始及替换指令，不修改游戏文件。
安装 `capstone` 和 `unicorn` 后，从仓库根目录运行：

```sh
python3 mhf/crates/mods/monster/tools/verify_native.py /path/to/mhfo-hd.dll
```

AI 边界回归使用相同依赖，要求上述 SHA-256 对应的 DLL：

```sh
python3 mhf/crates/mods/monster/tools/verify_ai_scan.py /path/to/mhfo-hd.dll
```

在 Unicorn 内执行原生 `0x10863750` / `0x10860A10`，验证缺失 `39 02` 的旧版
41 字节导出会陷入扫描循环，而完整 56 字节脚本返回后续分支。反编译器和绑定层
共用条件块检查，覆盖全部原生扫描调用者（包括固定宽度指令）；字节宽度校验不能
替代闭合关系校验。未闭合的 `native(...)` 草稿在分配／发布前被拒绝。
