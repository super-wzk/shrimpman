# 怪物 AI opcode 总表

本总表记录从本 crate 使用的固定 `mhfo-hd.dll`（镜像基址 `0x10000000`，
SHA-256 `95c580195f4080d2e9582c8c9df36abeb280476e088b6366583c5f138da8f301`）
中逆向出的 opcode 行为。它是逆向记录，不是作者词汇表。

## 覆盖

256 个 opcode 字节全部有归属。下面的数字由本目录的记录导出；119/256 的划分
和逐字节覆盖由 crate 的 `bytecode` 测试断言：

| 项目 | 数量 |
| --- | ---: |
| opcode 字节总数 | 256 |
| 被解释器 switch 分派的字节 | 137 |
| 这些字节到达的不重复 case 块 | 135 |
| 这些块调用的不重复 handler 例程 | 119 |
| 就地完成工作的 case 块 | 18 |
| 走 switch default 的字节（`0x00`、`0x43`、`0x6a..0x6f`、`0x87..0x8f`、`0x95..0x98`、`0x9d..0xfe`） | 119 |
| 操作已确证的记录（`confirmed`） | 93 |
| 操作已确证、但选择子集合是推断出来的记录（`operation_confirmed`） | 44 |
| 每个操作数角色都已确证的记录（`domain: named`） | 106 |
| 至少一个操作数仍只有地址的记录（`domain: generic`） | 31 |
| 仍未确证的操作数引用 | 51 |

## 信度分级

每条 opcode 记录两条相互独立的轴。

`confidence` 描述*操作*被钉死的程度：

- **confirmed** —— 解释器 case 块或它直接调用的 handler 证明了 opcode 做什么，
  包括写入的字段、发起的调用和游标移动。
- **operation_confirmed** —— 操作已证明，但某个选择子取值或分支条件是从别处
  重建的，而不是从一整块连续代码里读到的。
- **not_dispatched** —— 该字节没有跳转表项，走 default 路径。

`domain` 描述*操作数*被钉死的程度：

- **named** —— opcode 触碰的每个字段、表和全局都有已证明的角色。只有原生
  程序自己打印或存过的名字（`em->cmd_pl_target`、`maji_next_stage_no`、
  `EM_MODE_ATTACK`、`KIND_*` …）才会被当作游戏词汇使用。
- **generic** —— 操作已证明，但至少一个操作数只有地址。这些操作数列在
  `domain_unresolved` 里；`domain_notes` 记录后来证明的角色。
- **n/a** —— 该字节未被分派。

偏移是十进制字节偏移：默认从实体指针起（`+2544`），命名了别的指针时从那个
指针起（`global+9210`、`parent+1826`），或是绝对地址（`0x118C5784`）。这些
名字背后的字段字典是 [`runtime-fields.md`](runtime-fields.md)。

## 分派表如何验证

主解释器是 `0x108696D0`。它把 opcode 减一，与 `0xFE` 比较，索引字节表
`0x1086A304`，再通过指针表 `0x1086A0E4` 跳转；default 目标是 `0x1086A0BA`。
直接从镜像字节重建该查找（`index = byte[0x1086A304 + opcode - 1]`，
`target = dword[0x1086A0E4 + 4*index]`）能逐字节复现每条记录的 `dispatch`
地址，且每条记录的 `handler` 都是对应 case 块在返回分派循环前调用的子例程
之一。两项检查都对当前记录重跑过。

## 解释器共同事实

解释器接收 handler 返回的游标，每次调用最多执行 999 次分派；达到预算会写
`+3182 = 2`。`0x10860730` 是原生的指令宽度/跳过例程，`0x10860A10` 向前扫描
嵌套指令直到找到 opcode/选择子终止符；handler 的结构化条件形式用的后者。
`0x10860700` 选择活动游标 lane，`0x108614F0` 把动作续行存进 `+3288` 选中的
lane。

`0x1085C5E0` 递减 `+3228` 上的正延迟；只有它已经是零时才调用 `0x108696D0`
（前提是触发锁存 `+2601` 已置位），然后清掉锁存。它是 tick 门，不是 opcode。

### default 路径是停止，不是错误

上面列出的 119 个字节没有跳转表项。它们在 `0x1086A0BA` 的块是固定行为的真实
handler：当 `+2739`（命令模式）和 `+2659`（脚本状态）都为零时，它写
`+2659 = 2`，然后总是设置重启请求 `+2622 = 1`，通过 `0x10860430` 把游标
倒回，并离开分派循环。因此该字节是终止符：它从不被跳过，实体会以停机状态从
`main[0]` 重启。`src/ai/bytecode.rs` 把它建模为单字节指令加 `is_stop`，`Runtime`
也走同一条重置路径，而不是把该字节交给外部求值器。

### 事件选择子

`0x10860500` 的事件选择子按以下优先顺序消费 `+2823` 上的标志，并从对应根槽
装入第一个指针单元：

| mask | 根槽 | 保存游标字段 |
| ---: | ---: | ---: |
| `0x40` | `root + 56`（`root[14]`） | `+3288 = 0x40` |
| `0x80` | `root + 52`（`root[13]`） | `+3288 = 0x80` |
| `0x20` | `root + 16`（`root[4]`） | `+3288 \|= 0x20`、`+2604` |
| `0x10` | `root + 12`（`root[3]`） | `+3288 \|= 0x10`、`+2596` |
| `0x08` | `root + 44`（`root[11]`） | `+3288 \|= 0x08`、`+2644` |
| `0x04` | `root + 40`（`root[10]`） | `+3288 \|= 0x04`、`+2640` |
| `0x02` | `root + 32`（`root[8]`） | `+3288 \|= 0x02`、`+2636` |

`0x40` 和 `0x80` 两条 lane 在选择子中互斥。更低的 lane 可能先更新多个保存
游标，再选第一个非空游标。每行都是原生机制事实；它不赋予 DSL 层面的事件名。

## `0xff` 控制前缀

字节 `0xff` 和其他 opcode 一样被分派，但它的 handler `0x108675A0` 把后一个
字节读作控制选择子。`0x00..0x06` 和 `0xf5..0xff` 是命令；任何其他选择子字节
会被交回解释器按普通 opcode 执行，此时 `0xff` 只消耗自身。命令语义记录在
[`opcodes-ff.json`](opcodes-ff.json)：`0x00` reset、`0x01`/`0x02`
contents/sub-contents 返回、`0x03` route 返回、`0x04`/`0x05`/`0x06` 地面
区域移动、`0xf5` `UNKO_END`、`0xf6` `NO_FLOOR_END`、`0xf7` `FIND_NG_END`、
`0xf8`/`0xf9`/`0xfa` lane 清除、`0xfb` `AREA_END`、`0xfc` `FIND_END`、
`0xfd` `KEHAI_END`、`0xfe` `ROUTE_MOVE_END`、`0xff` 循环计数。

## 文件

- [`opcode-index.md`](opcode-index.md) —— 生成的 256 行索引（`opcode`、
  case 块、handler、操作、域、摘要）；
- [`opcode-matrix.md`](opcode-matrix.md) —— 本总表与 JSON 记录的速查版：
  按数值顺序的语义矩阵，以及按用途分类的作者视角；
- `opcodes-01-3f.json`、`opcodes-40-6f.json`、`opcodes-70-9c.json`、
  `opcodes-f6-ff.json` —— 已分派 opcode，含逐选择子变体；
- [`opcodes-default.json`](opcodes-default.json) —— 119 个 default 字节；
- [`opcodes-ff.json`](opcodes-ff.json) —— `0xff` 控制命令集；
- [`runtime-fields.md`](runtime-fields.md) —— 实体字段字典，以及镜像
  `domain_unresolved` 的"仍未命名"清单。

`opcode-index.md` 的 case 块、handler 与操作／域列取自 JSON 记录，摘要列与
`opcode-matrix.md` 的语义矩阵一致；改动记录后这两处要一起更新。
`runtime-fields.md` 里的残留清单必须与记录中的 `domain_unresolved` 数组保持
同步。

## 指令宽度

`src/ai/bytecode.rs` 编码了原生跳过例程 `0x10860730` 产生的宽度，包括它故意保留
的短形式。选择子读取器是共享的：`0x10860A10` 向前扫描，各 handler 对结构化
家族报告了不对称性，所以宽度表按选择子而不是按 opcode 验证。

相对第一版可移植解码器的重要修正是 `0x15`：

| 选择子 | 总宽度 |
| --- | ---: |
| `0` | 7 字节（`opcode + selector + 5 个载荷字节`） |
| `1` | 4 字节 |
| `2`、`3` | 2 字节 |

未知选择子按原生 fall-through 记录，而不是当作畸形拒绝，例如：

| 家族 | 未知选择子宽度 |
| --- | ---: |
| `0x0b/0x14/0x1b/0x22/0x32/0x36/0x42/0x5a/0x5e/0x63/0x64/0x69/0x71/0x74` | 2 |
| `0x0e/0x2b/0x34/0x3d/0x46/0x56/0x78/0x9a/0x9b/0x9c` | 2 |
| `0x15` | 2 |
| `0x1c/0x1d/0x20/0x23/0x27/0x2c/0x33/0x70/0x73/0x75/0x76/0x7a/0x7d` | 2 |
| `0x21` | 1 |
| `0x24` | 1 |
| `0x3e/0x57` | 1 |
| `0x79` | 2 |
| `0x80` | 1（`0xff` 除外，为 2） |
| `0x83` | 1（`0xff` 除外，为 2） |
| `0x94` | 2 |
| 每个 default 字节 | 1（终止符） |

## 仍未解决

剩余的缺口是*数据*角色，不是 opcode 行为：31 个已分派 opcode 引用 51 个操作数，
它们的角色还没有解释器之外的消费者证明。它们按 opcode 列在
`domain_unresolved` 里，并在 [`runtime-fields.md`](runtime-fields.md) 汇总。
给它们命名需要追踪产生方子系统（例如设置 `+2729` 的模块，或
`0x1E8001EC`/`0x1E7FFF3C` 背后的全局对象），而不是检查门本身。
