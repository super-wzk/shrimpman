# 怪物 AI opcode 语义矩阵

本文档是 [`opcode-catalog.md`](opcode-catalog.md) 与 `opcodes-*.json` 的速查
版，覆盖固定 `mhfo-hd.dll`（镜像基址 `0x10000000`，SHA-256
`95c580195f4080d2e9582c8c9df36abeb280476e088b6366583c5f138da8f301`）里解释器
`0x108696D0` 的全部 256 个 opcode 字节。

它是逆向记录，不是作者词汇表：表里的名字是描述性的，DSL 层面的可用形态见
[`dsl-spec.md`](dsl-spec.md)。

## 1. 覆盖统计

| 量 | 数量 |
| --- | ---: |
| opcode 字节总数 | 256 |
| 被解释器分派（有跳转表项） | 137 |
| 到达的 case 块 | 135 |
| case 块调用的不同 handler | 119 |
| 在 case 块里内联完成的 | 18 |
| 走 switch default 的字节 | 119 |
| 操作链已确证（`confirmed`） | 93 |
| 操作链已确证、选择子集合为推断（`operation_confirmed`） | 44 |
| 全部操作数角色已确证（`named`） | 106 |
| 至少一个操作数只按地址记录（`generic`） | 31 |
| 仍未命名的操作数引用 | 51 |

两条信度轴：

- **`confirmed`**：case 块或它直接调用的 handler 本身证明了 opcode 做什么，
  包括写入的字段、发起的调用和游标移动。
- **`operation_confirmed`**：操作已证明，但其中某个选择子取值或分支条件是
  重建出来的，而不是从一整块连续代码里直接读到的。
- **`not_dispatched`**：该字节没有跳转表项，走 default。

操作数轴：

- **`named`**：该 opcode 涉及的字段、表、全局都有已证明的角色。只有原生自己
  打印或存过的名字（`em->cmd_pl_target`、`maji_next_stage_no`、`EM_MODE_ATTACK`、
  `KIND_*` …）才会被当作游戏词汇使用。
- **`generic`**：操作已证明，但至少一个操作数只按地址记录，未证明数据角色。
- **`n/a`**：该字节未被分派。

偏移写法：`+2576` 是从实体指针起的十进制字节偏移，`global+9210` 表示另一
指针的相对偏移，`0x118C5784` 是绝对地址。字段含义见
[`runtime-fields.md`](runtime-fields.md)。

## 2. 阅读约定

- **选择子（selector）**：0x01–0x3F 与 0x70–0x9C 段大量 opcode 的第一个载荷
  字节是选择子。`选择子 0` 是"执行主体"的形式，`选择子 1/2`（有时到 3）是
  识别标记的续行或跳过形式，行为是"扫到本 opcode 的标记为止"。
- **标记块（marker block）**：条件不满足时，解释器不执行嵌套体，而是扫到
  同一个 opcode 字节的标记处继续，等价于"跳过一段"。条件不满足后"走嵌套体"
  与"扫过嵌套体"是两种相反的行为，表中分别写清。
- **lane**：运行时的并发执行通道，掩码在 `+3288`。位 `0x01` 是基 lane（跑
  状态表），`0x02`/`0x04`/`0x08`/`0x10`/`0x20` 是命令 lane，`0x40`/`0x80`
  是两条互斥事件 lane。
- **游标**：当前指令位置存在 `+2652`，各级保存游标在 `+2548`/`+2552`/`+2556`/
  `+2608`，lane 游标在 `+2588`/`+2596`/`+2604`。
- **目标域**：`+2612`/`+1826` 是**域内下标**，不固定指向某一个数组。已提交的命令
  kind（`+3231`/`+2581`）决定解析：kind 13 → `dword_1ED7AD2C + 3824*(slot % 40)`
  （em/actor 记录数组，解释器自身也在其中），其余 kind → `byte_1DC6B750 + 4176*slot`
  （玩家记录数组，仇恨表 `+2687`/`+2788`/`+2824`/`+2740` 都按同一个下标索引）。
  证据：`0x108538B0` case 1/13、`0x10855B30`、`0x10864A30`、`0x7E`（`0x108655B0`）。

## 3. 已分派 opcode（按数值升序）

| opcode | 名称 | 操作数 | 语义 | 域 | 信度 |
| --- | --- | --- | --- | --- | --- |
| `0x01` | mask-gated marker scan | 选择子 | 用 actor 的 lane 掩码对照配置的 lane 数量做门控，不通过则扫到下一个 `0x01` 标记；选择子 1/2 是识别标记的续行形式 | named | confirmed |
| `0x02` | tracked-player check | 选择子 | 0：`+2687` 非零执行正文；为零清当前目标 `+2612=FF` 并跳至 else/end；1：else；2：end | named | confirmed |
| `0x03` | target-id marker scan | 选择子 | 选择子 0 比较当前与待定的 16 位目标 id，目标不被接受时重置动作选择字段，再扫到 `0x03` 标记 | named | confirmed |
| `0x04` | reset-main-cursor | 无 | 把 `+2576`（主表下标）清 0，再装入 `main[0][0]`；不清 lane 掩码/delay/触发锁存 | named | confirmed |
| `0x05` | dispatch-action | 组、动作 id、参数（3 字节） | 读三字节动作元组，必要时调用动作分派器，并按续行标志保存续行游标 | named | confirmed |
| `0x06` | select-script-target | 模式 + 模式载荷 | 设置脚本选择模式及相应的组/下标字段；模式含字面对、归一化 16 位目标 id、当前 lane 目标、合格 actor 选择 | named | confirmed |
| `0x07` | main-index-jump | `u8` 下标 | 写 `+2576 = 下标`、清 `+2580`，把当前游标切到 `main[0][下标]`（状态表项） | named | confirmed |
| `0x08` | flag-0 marker gate | 选择子 | 选择子 0 在 `+1040`（实体状态字节）为 0 时直接返回，否则扫到 `0x08` 标记 | named | confirmed |
| `0x09` | flag-2 marker gate | 选择子 | 选择子 0 在 `+1040` 等于 2 时直接返回，否则扫到 `0x09` 标记 | named | confirmed |
| `0x0A` | set-selection-byte | `u8` | 把载荷字节写入 `+2088` | named | confirmed |
| `0x0B` | selection-byte comparison | 选择子 + `u8` | 选择子 0 把 `+2680`（mind 状态 a）拷进 `+2600` 并与载荷字节比较，不等则扫描 | named | confirmed |
| `0x0C` | set/clear selection flags | 2 字节 | 按第一个载荷字节选字段族做置位/清位，部分分支还受 actor 类型字节 `+3` 与 `+2739`（命令模式标志）门控 | named | confirmed |
| `0x0D` | clear-selection-flags | 选择子 | 清掉 `0x0C` 处理的同一批字段族；最后一个分支还把 `+2846`（保存的动作 id）置 -1 | named | confirmed |
| `0x0E` | target-angle/selection gate | 选择子 + 载荷 | 选择子 0 比较当前目标上下文与物种/actor 上下文，不满足则扫到 `0x0E` 标记；另有归一化目标 id 的快路径 | named | confirmed |
| `0x0F` | set-script-context | 5 字节 | 存四个上下文字节，标记续行上下文，并在表可用时按载荷第 3 字节选脚本表项 | named | confirmed |
| `0x10` | advance-script-choice | 无 | 递增脚本选择计数，按上下文模式 0/1/2 选下一个表项；模式 0 用 `+68` 作确定性下标 | named | confirmed |
| `0x11` | pick-highest-mask-lane | 无 | 扫 `+2821` 的置位，存最高 lane 下标，并设当前动作模式 1/组 0 | named | confirmed |
| `0x12` | pick-mask-lane-and-bind | 无 | 扫 `+2687`（仇恨标志），把最高 lane 同时存为当前下标与 `+2612`（cmd_pl_target），并清两个续行标志 | named | confirmed |
| `0x13` | bind-current-lane | 无 | 设动作模式 1/组 0，把 `+2612`（cmd_pl_target）当当前下标（非 -1 时掩到 4 位） | named | confirmed |
| `0x14` | angle-threshold gate | 选择子 + `u8` | 把载荷字节换算成角度阈值，算到所选目标上下文的相对角，超阈值则扫到 `0x14` 体标记 | named | confirmed |
| `0x15` | target-id list gate | 选择子 + 计数 + 大端 id 列表 | 选择子 0 读计数与大端目标 id 列表，逐个归一化，未命中则跳过嵌套 `0x15` 体 | named | confirmed |
| `0x16` | call_table9_subscript | `u8` | 调用 `root[9][index]`；无条件把续行保存到 `+2608`，由 `FF 03` 返回；不改变 stage，再次调用会覆盖续行；DSL 使用 table 9 函数 | named | confirmed |
| `0x17` | set-target-context | 4 字节 | 只在 `+2844`（区域移动计数）未置位时初始化四字节目标上下文：目标模式、计数、表选择子、上下文标志 | named | confirmed |
| `0x18` | advance-target-context | 无 | 目标上下文模式大于 1 且全局门允许时，标记上下文有效、调用目标选择、装入选中的脚本指针 | named | confirmed |
| `0x19` | copy-target-id-to-action | 无 | 把 `+3208`（maji_next_stage_no）拷进当前动作元组（模式 3/组/下标），并调用该路径使用的原生空钩子 | named | confirmed |
| `0x1A` | set-normalized-target-id | 大端 `u16` | 读大端 16 位目标 id，归一化后写入待定/当前目标与动作选择字段 | named | confirmed |
| `0x1B` | selection-byte equality gate | 选择子 + `u8` | 选择子 0 比较载荷字节与 `+2681`（mind 状态 b），否则扫到 `0x1B` 标记 | named | confirmed |
| `0x1C` | deterministic-ratio branch | 选择子 + 计数 + 阈值/体 | 选择子 0 用两个原生哈希式函数算确定性比值，与阈值列表比较，再走嵌套 `0x1C` 体 | named | confirmed |
| `0x1D` | ordered-byte branch | 选择子 + 计数 + 有序表 | 选择子 0 拿 `+2682` 与有序字节列表比较，跳过嵌套 `0x1D` 体直到第一个匹配或更大的项 | named | confirmed |
| `0x1E` | clear-behavior-requests | 无 | 清除已接受的行为请求、优先级、五类待处理标志及三类计时请求的已触发标志 | named | operation_confirmed |
| `0x1F` | flag-not-one gate | 选择子 | 选择子 0 仅在 `+1040` 不等于 1 时继续，否则扫到 `0x1F` 标记 | named | confirmed |
| `0x20` | relative-angle threshold branch | 选择子 + 阈值/列表 | 选择子 0 取目标位置，算相对 `+164`（朝向）的归一化相对角，与阈值表比较后走嵌套 `0x20` 体 | named | confirmed |
| `0x21` | counter-threshold gate | 选择子 | 选择子 0 仅在 `+2696` 大于 `+2708` 乘全局系数时继续，否则扫标记 | generic | confirmed |
| `0x22` | distance threshold gate | 选择子 + `u8` | 选择子 0 把 actor 位置与保存参考点 `+2852`/`+2856`/`+2860` 的距离与缩放后的阈值字节比较，超距离则跳过嵌套体 | named | operation_confirmed |
| `0x23` | actor-kind list gate | 选择子 + 计数 + 类型表 | 选择子 0 用最多 40 条 actor 记录比类型列表（含 `0xB1` 特例），无匹配则走嵌套 `0x23` 体 | named | confirmed |
| `0x24` | repeat-body counter | 选择子 + `u8` | 选择子 0 存重复次数与体游标；选择子 1 递减计数，仍为正则回到体 | named | confirmed |
| `0x25` | reset-repeat-counter | 无 | 清 `+2623`（解释器停止标志 2），也就是 `0x24` 用的重复计数器 | named | confirmed |
| `0x26` | set-secondary-selection-byte | `u8` | 把载荷字节写入 `+2738` | named | confirmed |
| `0x27` | ordered-runtime-byte branch | 选择子 + 计数 + 有序表 | 拿 `+27`（关联 actor 指针）与有序字节列表比较，按结果跳过嵌套 `0x27` 体 | generic | operation_confirmed |
| `0x28` | same-species-actor gate | 选择子 | 扫活动 actor 记录，找与 `+2040`（动作 id）同归一化物种的记录；没找到则走嵌套 `0x28` 体 | named | confirmed |
| `0x29` | positive-counter gate | 选择子 | 仅在带符号的 `+2910` 为正时继续，否则走嵌套 `0x29` 体 | named | confirmed |
| `0x2A` | positive-angle-counter gate | 选择子 | 仅在带符号的 `+2912` 为正时继续，否则走嵌套 `0x2A` 体 | generic | operation_confirmed |
| `0x2B` | field-equality branch | 选择子 + `u8` | 选择子 0 比较九个运行字段/actor 类型组合之一与载荷字节，不等则跳过嵌套 `0x2B` 体 | named | confirmed |
| `0x2C` | species-group branch | 选择子 + 计数 + 物种 ID | 按顺序把当前物种与各 case 物种经 `0x11A4E9C0` 归组后比较；首个同组 case 进入正文，无匹配则进入可选 else 或结束 | named | confirmed |
| `0x2D` | copy-action-context-7 | 无 | 设动作模式 7，把 `+2922`（两态字节 a）拷进当前动作组/下标字段 | named | confirmed |
| `0x2E` | select-context-table | `u8` | 由 actor 类型与载荷字节重算 `+1968`；特殊类型选不同的上下文表偏移与模式值 | named | confirmed |
| `0x2F` | available-lane gate | 选择子 | 在配置的 lane 里找低位可用标志非零的项；没找到则走嵌套 `0x2F` 体 | named | confirmed |
| `0x30` | mark-available-lane | 无 | 找到第一个低位可用标志非零的 lane，把 `+3190` 置 1 | named | confirmed |
| `0x31` | set-action-mode-8 | 无 | 把当前动作模式字节设为 8 | named | confirmed |
| `0x32` | selection-byte equality gate | 选择子 + `u8` | 选择子 0 比较 `+2088` 与载荷字节，不等则走嵌套 `0x32` 体 | named | confirmed |
| `0x33` | selection-byte list gate | 选择子 + 计数 + 表 | 选择子 0 拿 `+2088` 与载荷列表比较，直到命中前走嵌套 `0x33` 体 | named | confirmed |
| `0x34` | actor-kind-pair gate | 选择子 + 2 字节 | 选择子 0 比较 actor 的 `+20`/`+21` 字节与两个载荷字节，任一不同则走嵌套 `0x34` 体 | named | confirmed |
| `0x35` | rage-active condition | 选择子 | 0：`+2726` 怒态标志非零时执行正文，否则跳至 else/end；1：else；2：end | named | confirmed |
| `0x36` | distance threshold gate | 选择子 + `u8` | 选择子 0 算 actor 位置（`+172`/`+180`）与保存参考点（`+2852`/`+2860`）的距离，超阈值则走嵌套 `0x36` 体 | named | operation_confirmed |
| `0x37` | global-flag gate | 选择子 | 选择子 0 仅在全局对象标志 `+44` 不含位 `0x200000` 时继续，否则走嵌套 `0x37` 体 | named | confirmed |
| `0x38` | species-variant table gate | 选择子 | 选择子 0 按物种 id `+2040` 与变体 `+2008` 读表项，表项非零则抑制嵌套 `0x38` 体 | generic | operation_confirmed |
| `0x39` | flash-active condition | 选择子 | 0：`+2914` 闪光计时器非零时执行正文，否则跳至 else/end；1：else；2：end | named | confirmed |
| `0x3A` | nonzero-selection gate | 选择子 | 选择子 0 仅在 `+3186` 非零时继续，否则走嵌套 `0x3A` 体 | named | confirmed |
| `0x3B` | null-pointer gate | 选择子 | 选择子 0 仅在 `+3172`（关联 actor 指针）为空时继续，否则走嵌套 `0x3B` 体 | named | operation_confirmed |
| `0x3C` | other-actor gate | 选择子 | 选择子 0 只在 `+3172` 指向的 actor 其 `+2680` 等于 1 且物种 id 相同时抑制嵌套体 | named | operation_confirmed |
| `0x3D` | normalized-target-id gate | 选择子 + 大端 `u16` | 选择子 0 比较经 `0x10B4C5E0` 归一化的大端 id 与 `+2846`（保存的动作 id），不等则走嵌套 `0x3D` 体 | named | confirmed |
| `0x3E` | normalized-target-id list gate | 选择子 + 计数 + 大端 id 表 | 选择子 0 读计数与大端 id 列表，逐个归一化，直到命中前走嵌套 `0x3E` 体 | named | confirmed |
| `0x3F` | select-nearest-target-script | 大端 `u16` + `u8` | 读大端目标 id 与半径字节，解析目标表，选匹配或最近子项，装入脚本上下文与游标 | named | confirmed |
| `0x40` | set_runtime_mode | `u8` | 读一字节，为 0 时以 0、否则以 1 调用 `0x108693E0` | named | confirmed |
| `0x41` | conditional_angle_update | 选择子 | actor 类型 `+3` 为 27（或 28/31）且选择子为 0 时，要求三个谓词返回 0，算出缩放角度并调用 `0x10858E00` | generic | operation_confirmed |
| `0x42` | angle_threshold_branch | 选择子 + `u8` | 选择子 0 刷新向量，由后一字节得阈值，与 `0x10855830` 的绝对角差比较，按结果跨过 `0x42` 标记块 | generic | operation_confirmed |
| `0x44` | conditional_runtime_relation | 选择子 | 选择子 0 仅在 `+3172` 为空或其 `+2040` 与当前 `+2040` 不同时进入体；选择子 1 扫到终止符 | named | operation_confirmed |
| `0x45` | select_candidate_lane_by_flags | 选择子 | 选择子 0 扫四条全局记录（玩家域，`0x1DC6B750`，4176 步长），按活动掩码、当前 `+2040`、actor 类型 `+3`、记录标志过滤，按 `+2740` 浮点 lane 值排名，把选中下标写入 `+2612`（cmd_pl_target） | generic | operation_confirmed |
| `0x46` | conditional_lane_pair | 选择子 + 2 字节 | 选择子 0 读两字节对，与选中 lane 记录的 `+20`/`+21` 比较，不等则跨过 `0x46` 标记块 | named | confirmed |
| `0x47` | clear_one_shot_flag_or_wait | 选择子 | 选择子 0 清 `+2729`，非零时立即返回，否则扫 `0x47` 标记块 | generic | operation_confirmed |
| `0x48` | set_runtime_word_3228 | `u8` | 把载荷字节（0 视作 0）写入 `+3228`（延迟 tick） | named | confirmed |
| `0x49` | set_action_context_from_lane | `u8` | 用 `+2612`（cmd_pl_target）填当前动作字段；无选中 lane 时设类型 `+2581`=1、下标 `+2582`=0、值 `+2584`=-1，否则类型 11、下标 = lane 与 0xf | named | confirmed |
| `0x4a` | lane_mask_gate | 选择子 | 选择子 0 在选中 lane 位出现在 `+2684`（模式重置标志字节）且 lane 不为 `0xff` 时直接返回，否则扫 `0x4a` 标记块 | named | operation_confirmed |
| `0x4b` | clear_lane_value | 无 | 有选中 lane 时清 `+2788`（仇恨表 1）+ 4×(`+2612` & `0xf`) 处的 dword | named | confirmed |
| `0x4c` | clear_lane_timer | 无 | 有选中 lane 时清 `+2824`（仇恨表 2）+ 4×(`+2612` & `0xf`) 处的 dword | named | confirmed |
| `0x4d` | refresh_runtime_vectors | 无 | 以该对象调用 `0x108538B0` 刷新向量 | named | confirmed |
| `0x4e` | increase_runtime_value_2696 | 选择子 | 选择子 0 把 `+2708` 的一半加到 `+2696` 并夹到 `+2708` | generic | confirmed |
| `0x4f` | increase_runtime_value_2700 | 选择子 | 选择子 0 把 `+2712` 的一半加到 `+2700` 并夹到 `+2712`，再调 `0x10864790` | generic | confirmed |
| `0x50` | increase_runtime_value_2704 | 选择子 | 选择子 0 把 `+2716` 的一半加到 `+2704` 并夹到 `+2716` | generic | confirmed |
| `0x51` | runtime_phase_gate | 选择子 | 选择子 0 在 `+1040` 等于 4 时跳过体，否则扫 `0x51` 标记块 | named | confirmed |
| `0x52` | select_lane_priority_1 | `u8` | 玩家域候选 lane 搜索：按 `+2687`（仇恨标志）、`+1826`、`+2040`、`+2824` 与辅助谓词过滤，按 `+2740` 浮点值排名，把选中 lane 写入 `+2612` | named | operation_confirmed |
| `0x53` | select_lane_priority_2 | `u8` | 第二轮玩家域候选 lane 搜索：用活动掩码、匹配的 `+2040`/`+1252` 值、`+2824` 阈值、`+2740` 排名与专用辅助检查，写 `+2612`（cmd_pl_target） | generic | operation_confirmed |
| `0x54` | lane_record_gate | 选择子 | 选择子 0 在选中 lane 记录有效、其 `+2042` 为零且 `0x10A94CE0` 返回非零时直接返回，否则扫 `0x54` 标记块 | named | confirmed |
| `0x55` | lane_timer_gate | 选择子 | 选择子 0 在无选中 lane 或该 lane 的 `+2824` 小于 30000 时扫 `0x55` 标记块 | named | confirmed |
| `0x56` | global_word_gate | 选择子 + 大端 `u16` | 选择子 0 读大端 `u16`，与全局 `0x1E8001EC`+8 的字不等时扫 `0x56` 标记块 | generic | operation_confirmed |
| `0x57` | area_route_profile_selection | 选择子 + 载荷 | 按 `u8 +3185` 区域移动路线配置编号有序精确分支；`00 count` 开始，`01 value:u16be` case，`02` else，`03` 结束。字段来自生成记录 `+0x18`，用于选择物种/地图的区域路线表 | named | confirmed |
| `0x58` | copy_parent_lane_context | 选择子 | 设动作类型 `+2581`=1、下标 `+2582`=0；若 `+3172`（em 数组里的关联 actor 指针）存在则把其 `+1826`（该 actor 自己的目标槽）拷进 `+2584` 与 `+2612`，否则都置 -1 | named | operation_confirmed |
| `0x59` | runtime_flag_gate_3200 | 选择子 | 选择子 0 仅在 `+3200` 为零时扫 `0x59` 标记块 | generic | operation_confirmed |
| `0x5a` | lane_value_gate | `u8` | 选择子 0 在保存的动作类型 `+3231` 为 11 或 1 且选中 lane 时，若值等于 `0x1086A530`(lane`+2040`, lane`+2008`) 则立即返回 | named | confirmed |
| `0x5b` | clear_lane_words_by_mask | `u8` | 选择子为 0 时，对 `+2684` 里没有对应 lane 位的项，清 `+2688`（仇恨值）+ 2×i 处的字 | named | operation_confirmed |
| `0x5c` | world_position_gate | 选择子 | 选择子 0 在 `+2040` 等于 `global+20` 且 `0x108CF5B0`(`+172`, …) 成功时立即返回，否则扫 `0x5c` 标记块 | named | confirmed |
| `0x5d` | vector_initialized_gate | 选择子 | 选择子 0 刷新向量，`+2852`/`+2856`/`+2860` 三个浮点都非零则返回，否则调用 `nullsub_2` 并扫 `0x5d` 标记块 | named | operation_confirmed |
| `0x5e` | species_value_gate | `u8` | 选择子 0 拿载荷字节与 `0x1086A530`(`+2040`, `+2008`) 比较，并对表值 365/367/369 有特殊处理 | generic | operation_confirmed |
| `0x5f` | select_lane_by_species | `u8` | 选择子驱动 lane 搜索：按 `+2687`（仇恨标志）、活动记录、匹配 `+2040` 与辅助分类过滤，按 `+2740` 或 `+2824` 排名后写 `+2612` | generic | operation_confirmed |
| `0x60` | global_flag_gate_9466 | 选择子 | 选择子 0 仅在全局字节 `0x1E7FFF3C`+9466 为 0 时扫 `0x60` 标记块 | generic | operation_confirmed |
| `0x61` | resolve_event_table_value | 无 | 用全局表 `0x118C7308` 中由 `+3` 选出的指针调用 `0x1085C170`；反编译里有一处从临时量写 `+2912` 的痕迹，只作为未验证记录保留 | generic | operation_confirmed |
| `0x62` | conditional_marker_98_gate | 选择子 + 2 字节 | 选择子 0 跳过两字节载荷，按辅助函数得出的选择子反复扫 `0x62`/`0x98` 标记；选择子 1..3 扫到 `0x62` 终止符 | named | confirmed |
| `0x63` | runtime_bitmask_gate | `u8` | 选择子 0 读掩码字节，(掩码 & `+27`) 为 0 时扫 `0x63` 标记块 | generic | operation_confirmed |
| `0x64` | lane_threshold_gate | `u8` | 选择子 0 读索引字节，无选中 lane 或 lane 计时器 `+2824` 低于全局阈值表 `0x11A4BFB0[index]` 时扫 `0x64` 标记块 | named | operation_confirmed |
| `0x65` | clear_lane_timers | 无 | 清 `+2824`（仇恨表 2）起连续四个 dword：`+2824`/`+2828`/`+2832`/`+2836` | named | confirmed |
| `0x66` | all_records_empty_gate | 选择子 | 选择子 0 仅在全局记录集全部为空时扫 `0x66` 标记块 | named | confirmed |
| `0x67` | runtime_flag_2739_gate | 选择子 | 选择子 0 在 `+2739`（命令模式标志）为 0 时立即返回，否则扫 `0x67` 标记块 | named | confirmed |
| `0x68` | request_runtime_refresh | 无 | 当 `+2739`（命令模式标志）与 `+2659` 都为 0 时，置 `+2659`=2、`+2622`=1 并调用 `0x10860430` | named | confirmed |
| `0x69` | short_angle_threshold_branch | `u8` | 选择子 0 刷新向量，由后一字节缩放得阈值，与绝对角差比较后走 `0x69` 标记分支；另要求阈值小于 `0x4000` | generic | operation_confirmed |
| `0x70` | ordered_selector_scan | 选择子 + 计数 | 选择子 0 读计数，把列表字节与 `+3`（物种 id）比较，扫过或跳过嵌套 `0x70` 块；选择子 1/2 扫到选择子 3 | named | confirmed |
| `0x71` | threshold_selector_gate | 选择子 + `u8` | 选择子 0 把载荷字节与带符号 `+3212`（带符号阈值）比较，不满足则扫嵌套 `0x71` 块 | named | confirmed |
| `0x72` | global_guard_selector_gate | 选择子 | 选择子 0 由全局 `0x1E8001EC`+8、`0x1ED52870`、`0x1ED52951` 门控后扫嵌套 `0x72` 块 | generic | operation_confirmed |
| `0x73` | masked_runtime_selector_scan | 选择子 + 计数 | 选择子 0 把列表与 `+2920`（目标标志字）按 `0x7f` 掩码后比较，相等时清掉保留高位的字段 | named | confirmed |
| `0x74` | masked_runtime_threshold_gate | 选择子 + `u8` | 选择子 0 把载荷字节与 `+2920`（目标标志字）按 `0x7f` 掩码后比较；受保护的辅助函数可让条件直接成立 | generic | operation_confirmed |
| `0x75` | ordered_runtime_2930_scan | 选择子 + 计数 | 选择子 0 把列表字节与 `+2930`（`0x75` 比较字节）比较，跳过或扫过嵌套 `0x75` 块 | generic | operation_confirmed |
| `0x76` | global_flag_ordered_scan | 选择子 + 计数 | 选择子 0 由全局 `0x1E8001EC`+2357 的位推出比较字节，比较列表后扫嵌套 `0x76` 块 | generic | operation_confirmed |
| `0x77` | global_bit_gate | 选择子 | 选择子 0 仅在全局字节 `0x1ED6BCA0` 置位 `0x10` 且清位 `0x08` 时继续 | generic | operation_confirmed |
| `0x78` | angle_interval_gate | 选择子 + 2 字节 | 选择子 0 读上下界字节，由 `+2068`（角度来源）与 `+164` 算当前角，条件成立才扫嵌套 `0x78` 块 | named | confirmed |
| `0x79` | callback_result_ordered_scan | 选择子 + 计数 + 参数 + 列表 | 选择子 0 用载荷字节 2 调 `+1140`（行为对象指针）+12 的函数指针，把返回字节与从偏移 4 开始的有序表比较 | named | confirmed |
| `0x7a` | mapped_global_ordered_scan | 选择子 + 计数 | 选择子 0 把全局 `0x1E7FFF3C`+52 经表 `0x118648F0` 映射后比较列表，再扫嵌套 `0x7a` 块 | generic | operation_confirmed |
| `0x7b` | increment_runtime_1096 | 无 | 递增 `+1096`（脚本 RNG 计数），只消耗 opcode 字节 | named | confirmed |
| `0x7c` | mapped_global_gate | 选择子 | 选择子 0 把全局 `0x1E7FFF3C`+52 经 `0x118648F0` 映射，映射值为 9/10/53/54 时直接继续，否则扫嵌套 `0x7c` 块 | generic | operation_confirmed |
| `0x7d` | actor_type_list_gate | 选择子 + 计数 + 列表 | 选择子 0 读计数与候选字节，用 `0x1087CB30`(`+3`, 候选) 逐个测试，未命中前跳过嵌套 `0x7d` 块 | named | confirmed |
| `0x7e` | candidate_lane_selection | 选择子 | 清 `+2582` 与 `+2612`；先按玩家域 lane 搜索 `sub_10864D90`，未命中再切到 em 域：在 `dword_1ED7AD2C`（3824 步长，最多 40 条）里挑"非自身、活动、`+3187`≠0、`+2040` 与自身相同、且在半径/高度内"的记录，把该记录自己的下标 `+12` 写进 `+2612`/`+2584`，命令 kind 置 13 | generic | operation_confirmed |
| `0x7f` | bit_40_clear_gate | 选择子 | 选择子 0 仅在 `0x1086A920` 返回 0 时清 `+1042`（实体状态字）的位 `0x40`，然后扫嵌套 `0x7f` 块 | named | confirmed |
| `0x80` | bounded_nested_delta_scan | 选择子 + 计数 | 选择子 0 读计数，扫嵌套 `0x80` 记录，把每条第 3 字节的带符号增量按 `+1096` 与 31 偏移累加 | named | confirmed |
| `0x81` | call_primary_subscript | `u8` | 调用 `root[1][index]`；从 stage 0 调用时保存 `0x81` 后续行游标到 `+2552`，同层调用则是尾跳转；置 stage=1 | named | confirmed |
| `0x82` | call_secondary_subscript | 2×`u8` | 调用 `root[15 + group][index]`；从 stage 1 调用时保存 `0x82` 后续行游标到 `+2556`，同层调用则是尾跳转；置 stage=2 | named | confirmed |
| `0x83` | lane_value_ordered_skip | 选择子 + 计数 | 选择子 0 把计数列表与选中 lane 值比较（`+3231` 为 13 时改用距离推导值），把值写 `+1112`，并把跳过计数存 `+2586` | generic | operation_confirmed |
| `0x84` | increment_runtime_1096 | 无 | 与 `0x7b` 共用内联递增路径：递增 `+1096`（脚本 RNG 计数） | named | confirmed |
| `0x85` | set_runtime_flag_40 | 无 | 置 `+1042`（实体状态字）位 `0x40` 并调用 `0x113AAAC0` | named | operation_confirmed |
| `0x86` | clear_runtime_flag_40 | 无 | 清 `+1042`（实体状态字）位 `0x40` | named | confirmed |
| `0x90` | load_vector_triplet | `u8` | 用载荷字节索引 `0x118C5784[10×(`+3`)]`，把三个浮点拷进 `+172`/`+176`/`+180` 与 `+1784`/`+1788`/`+1792` | named | operation_confirmed |
| `0x91` | compute_angle_to_field_164 | `u8` | 由 `+172` 起的向量算出无符号 16 位结果存进 `+164`（朝向），并消耗载荷字节 | named | confirmed |
| `0x92` | consume_only | 无 | 除消耗 opcode 字节外没有观察到其他动作 | named | confirmed |
| `0x93` | consume_only | 无 | 与 `0x92` 相同的空实现 | named | confirmed |
| `0x94` | global_value_ordered_scan | 选择子 + 计数 | 选择子 0 把计数列表与全局 dword `0x11C6A7E8` 比较并扫嵌套 `0x94` 块 | named | operation_confirmed |
| `0x99` | set_field_164_high_byte | `u8` | 把载荷字节左移 8 位写入 dword `+164`（朝向），共消耗 2 字节 | named | confirmed |
| `0x9a` | scaled_runtime_threshold_gate | 选择子 + `u8` | 选择子 0 把载荷字节按全局浮点 `0x119B5EF0` 与 `+2712` 缩放，与 `+2700` 比较后条件性跳过嵌套 `0x9a` 块 | generic | operation_confirmed |
| `0x9b` | selected_record_gate | 选择子 | 选择子 0 用原始记录标志、辅助函数与记录偏移 `+1040` 检查 `+1826`（玩家域下标，解析到 `0x1DC6B750 + 4176*slot`）处的选中记录，全部通过则立即返回 | named | confirmed |
| `0x9c` | action_context_distance_gate | 选择子 + `u8` | 选择子 0 把载荷字节放大 100 倍，仅当动作字段 `+2581`/`+2582`/`+2584` 与 `+2856`/`+1800` 距离满足观察到的谓词时才穿过嵌套体 | named | operation_confirmed |
| `0xff` | control_prefix | `u8` 选择子 | 控制前缀：后一字节是选择子；`0x00..0x06` 与 `0xf5..0xff` 是命令，其余选择子交回解释器按普通 opcode 执行（此时 `0xff` 只消耗自身） | named | confirmed |

## 4. `0xff` 控制命令（选择子）

| 选择子 | 名称 | 语义 |
| --- | --- | --- |
| `0x00` | reset | 重装主表第 0 项，清活动 lane 掩码并标记脚本停止：`+2576`=0、stage `+2580`=0、游标 `+2652`=main[0]、`+3288`=0、`+2659`=2 |
| `0x01` | contents_return | 回到保存的 contents 游标，并把选择 stage 切回 0（contents）；原生日志 `CONTENTS TBL END` |
| `0x02` | sub_contents_return | 回到保存的 sub-contents 游标，把 stage 切到 1；原生日志 `SUB CONTENTS TBL END` |
| `0x03` | route_return | 回到第三个保存游标 `+2608` |
| `0x04` | ground_area_move | 推进当前 act 的地面区域移动记录：按 `+2040` 在 `+2900`→`+24` 的 act 列表里查表，夹取 act/步进值 |
| `0x05` | ground_area_move | 与 `0x04` 同一条实现（原生两块共用一个代码块） |
| `0x06` | ground_area_move_snap | 地面区域移动变体：保存的命令种类为移动态（9）时直接跳到步进放置路径 |
| `0xf5` | unko_end | 硬重置到第一条主表项：`+2576`=0、stage=0、游标=main[0][0]、`+3288`=0、`+2659`=2；原生日志 `UNKO_END` |
| `0xf6` | no_floor_end | 与 `0x00` 相同的重置，用于路由没有地面时；原生日志 `NO_FLOOR_END` |
| `0xf7` | find_ng_end | 结束目标搜索：重置主选择，再清 `+2612`（cmd_pl_target）所持槽位的仇恨簿记；`+2612` 为 `0xFF` 时按 `NOT_FOUND_PL` 结束 |
| `0xf8` | clear_lane8 | 清 lane 位 `0x0008`；若 lane 位 `0x0001` 仍在则恢复 `main[2][+2595]`，否则走完整重置 |
| `0xf9` | clear_lane4 | 清 lane 位 `0x0004`；同上按 `0x0001` 决定恢复还是重置 |
| `0xfa` | clear_lane2 | 重装主选择，清 lane 位 `0x0002` 并调用 `0x1084FB80`(em, 0)；再按 `0x0001` 决定恢复 `main[2][+2595]` 或重置 |
| `0xfb` | area_end | 结束地面区域移动：比较进度计数 `+2844` 与上限 `+2845`，把保存游标 `+2616` 作为下一游标并清等待标志 `+2621`；原生日志 `AREA_END` |
| `0xfc` | find_end | 结束搜索/目标命令：`+2739`（命令模式标志）为 0 时经 `0x108693E0`(em, 1) 抬触发器锁存并把 `+2680` 拷入 `+2681`；原生日志 `FIND_END Em_Mode_Chg( em, EM_MODE_ATTACK )` |
| `0xfd` | kehai_end | 结束気配路由：清 lane 位 `0x0010`；lane 位 `0x0001` 仍在时按 `+2595` 的 route act 恢复 `main[2][+2595]`（`route_ptr_set`），否则重装主项并置网络命令状态（`em_cmd_top` + `EM_NET_CMD`） |
| `0xfe` | route_move_end | 结束路由移动：刷新参考位置，按物种选半径比较距离，未到达则恢复 lane 游标；到达则推进计数 `+2842`，按模式 `+2592` 选下一个航点（0 = 用 `+1096` 在 `+2843` 范围取伪随机，1 = 递增，2 = 取 `main[2][+2593]` 的字节） |
| `0xff` | loop_count | 路由移动的循环计数：`+2842` 低于上限 `+2841` 时恢复 `main[2][+2594]`；达到上限则清 lane 位 `0x0001`，由 lane 选择器 `0x10860700` 决定下一游标 |
| 其他 | re-dispatch | 未出现在两张 switch 表（`0x108675D4` 覆盖 `0x00..0x06`、`0x10867CDC` 覆盖 `0xf6..0xff`）里的选择子，交回解释器按普通 opcode 执行 |

注意 `0xff` 家族里 `0x04`/`0x05` 共用一条实现，`0x01`/`0x02`/`0x03` 是三级
游标返回，`0xf8`/`0xf9`/`0xfa`/`0xfd` 是"清 lane 位 + 视基 lane 是否仍在决定
恢复或重置"的同一模式。

## 5. 未分派字节（终止语义）

以下 119 个字节没有跳转表项，走 switch default `0x1086A0BA`：

```text
0x00, 0x43, 0x6a..0x6f, 0x87..0x8f, 0x95..0x98, 0x9d..0xfe
```

default 不是错误也不是未实现：在 `+2739`（命令模式标志）与 `+2659` 都为 0 时
它会写 `+2659`=2，总是写 `+2622`（重启请求）=1，然后调用 `0x10860430` 把游标
回卷到 `main[0]` 并退出本 tick 的分派。也就是说这些字节的语义是"停止并复位"。

顺带一提，`0x00` 也在这个集合里，所以"脚本开头是 0x00"与"脚本走到 0x9d"是
同一种行为。

## 6. 按用途分类（作者视角）

下面这层是**用法建议**，不是逆向事实：

| 用途 | opcode |
| --- | --- |
| 流程与游标：直接决定下一步走哪 | `0x04`、`0x05`、`0x07`、`0x10`、`0x16`、`0x18`、`0x19`、`0x24`、`0x25`、`0x81`、`0x82`、`0xff` 家族 |
| 条件门：按状态决定是否跳过一段 | `0x01`–`0x03`、`0x08`–`0x0F`、`0x14`、`0x15`、`0x1B`–`0x1D`、`0x1F`–`0x23`、`0x27`–`0x3E`、`0x40`–`0x9C` 中的判定类 |
| 目标与仇恨簿记 | `0x11`–`0x13`、`0x1A`、`0x3F`、`0x49`–`0x5C`、`0x7E` |
| 位置、朝向、向量 | `0x0E`、`0x20`、`0x36`、`0x42`、`0x4D`、`0x5C`、`0x5D`、`0x69`、`0x78`、`0x90`、`0x91`、`0x99` |
| 状态字与标志位 | `0x0A`、`0x0C`、`0x0D`、`0x26`、`0x31`、`0x48`、`0x7F`、`0x85`、`0x86` |
| 空实现（只占位） | `0x92`、`0x93` |

DSL 支持的动作、状态转移、条件分支和返回语法见 [`dsl-spec.md`](dsl-spec.md)。
原生指令已识别不代表 DSL 已支持；例如 `0x24` 的执行与扫描边界不一致，仍拒绝安装。

## 7. 仍然存在的缺口

- 31 个 `generic` opcode 与 51 个未命名操作数：缺的是"数据角色"证明，不是
  行为证明。它们涉及的全局表 `0x118648F0`、`0x1188EAB0`、`0x11A4E9C0`、
  `0x11406AF0` 的行含义还没追到产生方。
- `0x61` 有一处从临时量写 `+2912` 的反编译痕迹，未验证，按原样记录。
- 事件 lane（`0x40`/`0x80`）的收尾命令尚未确证。
