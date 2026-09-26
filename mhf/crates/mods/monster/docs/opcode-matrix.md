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
| 操作链已确证（`confirmed`） | 101 |
| 操作链已确证、选择子集合为推断（`operation_confirmed`） | 36 |
| 全部操作数角色已确证（`named`） | 100 |
| 至少一个操作数只按地址记录（`generic`） | 37 |
| 仍未命名的操作数引用 | 61 |

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
- **lane**：运行时的执行通道，掩码在 `+3288`。位 `0x01` 是基路线 lane（使用
  `root[2]`），`0x02`/`0x04`/`0x08`/`0x10`/`0x20` 是命令 lane，`0x40`/`0x80`
  是两条互斥事件 lane。
- **游标**：当前指令位置存在 `+2652`，各级保存游标在 `+2548`/`+2552`/`+2556`/
  `+2608`，lane 游标在 `+2588`/`+2596`/`+2604`。
- **目标域**：`+2612`/`+1826` 是**域内下标**，不固定指向某一个数组。已提交的命令
  kind（`+3231`/`+2581`）决定解析：kind 13 → `dword_1ED7AD2C + 3824*slot`
  （em/actor 记录数组，解释器自身也在其中），玩家 kind 1/11 → `byte_1DC6B750 + 4176*slot`
  （玩家记录数组；追踪掩码、察觉分、仇恨分和距离量分别按同一下标维护）。
  证据：`0x108538B0` case 1/13、`0x10855B30`、`0x10864A30`、`0x7E`（`0x108655B0`）。

## 3. 已分派 opcode（按数值升序）

| opcode | 名称 | 操作数 | 语义 | 域 | 信度 |
| --- | --- | --- | --- | --- | --- |
| `0x01` | mask-gated marker scan | 选择子 | 用 actor 的 lane 掩码对照配置的 lane 数量做门控，不通过则扫到下一个 `0x01` 标记；选择子 1/2 是识别标记的续行形式 | named | confirmed |
| `0x02` | tracked-player check | 选择子 | 0：`+2687` 非零执行正文；为零清当前目标 `+2612=FF` 并跳至 else/end；1：else；2：end | named | confirmed |
| `0x03` | current-pending-area gate | 选择子 | 选择子 0 比较当前区域 `+2040` 与待定区域 `+2046`，区域目标不被接受时重置动作上下文并扫描 else/end | named | confirmed |
| `0x04` | reset-main-cursor | 无 | 把 `+2576`（主表下标）清 0，再装入 `main[0][0]`；不清 lane 掩码/delay/触发锁存 | named | confirmed |
| `0x05` | dispatch-action | 组、动作 id、参数（3 字节） | 读三字节动作元组，必要时调用动作分派器，并按续行标志保存续行游标 | named | confirmed |
| `0x06` | select-script-target | 模式 + 模式载荷 | 配置动作目标 kind、组和下标；可选择玩家、区域、路线点、物种固定点、缓存物件点、路径步及其他怪物；后续由目标解析器消费，DSL 仅覆盖规范子集 | named | confirmed |
| `0x07` | main-index-jump | `u8` 下标 | 写 `+2576 = 下标`、清 `+2580`，把当前游标切到 `main[0][下标]`（状态表项） | named | confirmed |
| `0x08` | flag-0 marker gate | 选择子 | 选择子 0 在 `+1040`（实体状态字节）为 0 时直接返回，否则扫到 `0x08` 标记 | named | confirmed |
| `0x09` | flag-2 marker gate | 选择子 | 选择子 0 在 `+1040` 等于 2 时直接返回，否则扫到 `0x09` 标记 | named | confirmed |
| `0x0A` | write-reaction-state-byte | u8 | 整字节写 `+2088`，影响模式转换、感知和异常状态；不是 Mode 字段 `+2680`，完整取值语义未定 | generic | confirmed |
| `0x0B` | mode comparison | 选择子 + u8 | 选择子 0 保存模式 `+2680` 到快照 `+2600` 后与参数比较，不等则跳过正文；DSL：`self.mode_is(Mode::Normal/Attack)` | named | confirmed |
| `0x0C` | write-selected-runtime-byte | 字段选择子 + u8 值 | 按选择子整字节赋值；0 遍历 1..10，1..4 为公共字段，5..9 带物种及本地命令门控，10 无写入；不是位掩码操作。DSL 的 handle 仅使用 `0C 04 01` | generic | confirmed |
| `0x0D` | clear-selected-runtime-field | 字段选择子 | 清零对应字段；选择子 10 将保存的区域／路线上下文 ID `+2846` 置 FFFF；0 批量处理。`0D 04` 仅清接管，紧接本函数返回才可恢复为 pass | generic | confirmed |
| `0x0E` | in_area | 选择子；0 带 u16 大端区域 ID | 参数经原生地图／昼夜适配后与当前区域 `+2040` 比较，相等进入正文；DSL：`self.in_area(id)` | named | confirmed |
| `0x0F` | start-route-context | 模式、次数、源脚本、入口脚本、动作脚本（5 字节） | 建立路线移动上下文，保存活动 lane 续行并启用基 lane；查当前区域点表、记录次数／范围，跳入 `root[2][payload[3]]`，不是单纯写配置 | named | confirmed |
| `0x10` | bind-route-waypoint | 无 | 路线模式 0..2 下递增 `+3250`，仅在当前路线点无效／越界时重选；模式 0 用 `+1096 % 点数`，1 读 `root[2][+2593]` 首字节，2 选 0；最后绑定 kind 2/group 1 的路线点，不是每次推进一点 | named | confirmed |
| `0x11` | pick-highest-mask-lane | 无 | 扫 `+2821` 的置位，存最高 lane 下标，并设当前动作模式 1/组 0 | named | confirmed |
| `0x12` | pick-mask-lane-and-bind | 无 | 扫 `+2687`（已追踪玩家掩码），把最高 lane 同时存为当前下标与 `+2612`（cmd_pl_target），并清两个续行标志 | named | confirmed |
| `0x13` | bind-current-lane | 无 | 设动作模式 1/组 0，把 `+2612`（cmd_pl_target）当当前下标（非 -1 时掩到 4 位） | named | confirmed |
| `0x14` | angle-threshold gate | 选择子 + `u8` | 把载荷字节换算成角度阈值，算到所选目标上下文的相对角，超阈值则扫到 `0x14` 体标记 | named | confirmed |
| `0x15` | area_match | 选择子 + 计数 / u16 大端 case | 按地图适配后的区域 ID 匹配 actor+2040；按源码顺序首次匹配；DSL 为 `match self.area`，else 可省略 | named | confirmed |
| `0x16` | call_table9_subscript | `u8` | 调用 `root[9][index]`；无条件把续行保存到 `+2608`，由 `FF 03` 返回；不改变 stage，再次调用会覆盖续行；DSL 使用 table 9 函数 | named | confirmed |
| `0x17` | initialize-area-route | 区域列表、次数、换区脚本、上下文字节（4 字节） | 仅在 `+2844 == FF` 时初始化 root[5] 区域列表及 root[6] 处理脚本等字段，清选择下标 `+2845=FF`；次数 <=1 恢复未初始化标记；不开始换区 | generic | confirmed |
| `0x18` | try_change_area | 无 | 路线条目数大于 1 且全局门允许时，选择目的区域、保存续行并转入 root[6] 配置脚本；DSL 为 self.try_change_area()，不保证立即完成换区 | named | confirmed |
| `0x19` | bind-next-area | 无 | 将已选择的下一地区 `+3208`（maji_next_stage_no）拷入 kind 3 的区域动作目标并调用空钩子，不选择新区域、不换区 | named | confirmed |
| `0x1A` | set-next-area | u16 大端区域 ID | 适配区域 ID，写待定区域 `+2046`、下一地区 `+3208` 及 kind 3 目标；不写当前区域 `+2040`、`+2038` 或位置 | named | confirmed |
| `0x1B` | accepted-request flag equality | 选择子；0 带 u8 | 比较已接受请求标志 `+2681` 与参数，不等则扫描 else/end；handle 使用值 1 的形式，不是独立 mind 状态测试 | named | confirmed |
| `0x1C` | deterministic-ratio branch | 选择子 + 计数 + 阈值/体 | 选择子 0 用两个原生哈希式函数算确定性比值，与阈值列表比较，再走嵌套 `0x1C` 体 | named | confirmed |
| `0x1D` | request-dispatch | 选择子 + 计数 + 有序表 | 选择子 0 按当前已被接受的请求编号 `+2682` 做有序分支：等于则进正文，小于 case 值则放弃整块，大于则跳到下一个 case。DSL：`match self.request` | named | confirmed |
| `0x1E` | clear-behavior-requests | 无 | 清除已接受的行为请求、优先级、五类待处理标志及三类计时请求的已触发标志 | named | operation_confirmed |
| `0x1F` | flag-not-one gate | 选择子 | 选择子 0 仅在 `+1040` 不等于 1 时继续，否则扫到 `0x1F` 标记 | named | confirmed |
| `0x20` | relative-angle threshold branch | 选择子 + 阈值/列表 | 选择子 0 取目标位置，算相对 `+164`（朝向）的归一化相对角，与阈值表比较后走嵌套 `0x20` 体 | named | confirmed |
| `0x21` | counter-threshold gate | 选择子 | 选择子 0 仅在 `+2696` 大于 `+2708` 乘全局系数时继续，否则扫标记 | generic | confirmed |
| `0x22` | near-target-2d-gate | 选择子 + `u8` | 选择子 0 用**水平**（x/z）距离比较 actor `+172`/`+180` 与参考点 `+2852`/`+2860`，阈值取 `max(n×100, 体型×缩放+60)`；不超过则进入正文（忽略高度）。DSL：`self.near_target_2d(n)` | named | operation_confirmed |
| `0x23` | actor-kind list gate | 选择子 + 计数 + 类型表 | 选择子 0 用最多 40 条 actor 记录比类型列表（含 `0xB1` 特例），无匹配则走嵌套 `0x23` 体 | named | confirmed |
| `0x24` | repeat-body-counter | 选择子；0 带 i8 次数 | 唯一的 `+2623/+2632` 保存重复次数／体游标；尾部先减计数，为正才回跳，非正初值走特殊扫描；执行／扫描宽度不一致，当前含 native 在内均拒绝安装 | named | confirmed |
| `0x25` | reset-repeat-counter | 无 | 清重复计数 `+2623`，不跳出正文；随后 `24 01` 将 0 减为 FF 并不再循环，不等于 break | named | confirmed |
| `0x26` | write-automatic-response-gate | u8 | 整字节写 `+2738`；部分自动事件要求等于 0，感知／自动切攻击路径要求不等于 1，不能将全部取值归为布尔 | generic | confirmed |
| `0x27` | ordered-runtime-byte branch | 选择子 + 计数 + 有序表 | 比较未定角色的运行字节 `+27` 与有序列表，决定跨过哪些嵌套分支；不把该字节命名为关联 actor 指针 | generic | operation_confirmed |
| `0x28` | player-in-area gate | 选择子 | 扫活动玩家记录，找与自身 `+2040`（当前区域 id）相同的记录；找到则进入条件体，否则走 else 或结束。不做地图／昼夜映射，也不检查追踪或距离 | named | confirmed |
| `0x29` | area_timer_expired | 选择子 | signed i16 `+2910 <= 0` 进入正文，正值跳至 else 或结束；只检查区域相关倒计时，不等待或触发换区；DSL 为 `self.area_timer_expired` | named | confirmed |
| `0x2A` | attack_timer_active | 选择子 | signed i16 `+2912 > 0` 进入正文，否则跳至 else 或结束；DSL 为 `self.attack_timer_active`，不检查当前模式 | named | confirmed |
| `0x2B` | field-equality branch | 选择子；0 带字段选择子和值 | 选择字段、按物种条件比较载荷字节，不相等则扫描 else/end；部分字段与 0C/0D 共用，包括 `+1364/+1365`；handle 使用接管字段形式 | generic | confirmed |
| `0x2C` | species-group branch | 选择子 + 计数 + 物种 ID | 按顺序把当前物种与各 case 物种经 `0x11A4E9C0` 归组后比较；首个同组 case 进入正文，无匹配则进入可选 else 或结束 | named | confirmed |
| `0x2D` | bind-scanned-object | 无 | 把本帧物件扫描记录的全局 32 槽地面物件提交为动作目标（`+2581=7`，`+2582/+2584` 取 `+2922/+2923`）；本身不筛选、不移动、不执行动作 | named | confirmed |
| `0x2E` | select-perception-profile | `u8` | 把 `actor+1968` 指向该物种感知参数表的第 `n` 条 32 字节记录（最大/最小距离、高度带、半视角、两个阈值）；下标为物种私有索引，无范围检查，且可被物种条件改写 | named | confirmed |
| `0x2F` | any_player_carrying | 选择子 | 任一配置玩家的搬运状态低四位非零时进入正文，否则跳至 else 或结束；DSL 为 `context.any_player_carrying` | named | confirmed |
| `0x30` | request-player-carry-response | 无 | 任一配置玩家的搬运状态低四位非零则置待处理请求 `+3190=1`，随后由调度器按优先级 0x70 处理；不选目标、不查同区／距离、不立即执行响应 | named | confirmed |
| `0x31` | bind-cached-object-point | 无 | 只设目标 kind `+2581=8`，后续解析使用物件扫描缓存点 `+2864/+2868/+2872`；不是出生点／归巢点 | named | confirmed |
| `0x32` | selection-byte equality gate | 选择子 + `u8` | 选择子 0 比较 `+2088` 与载荷字节，不等则走嵌套 `0x32` 体 | generic | confirmed |
| `0x33` | selection-byte list gate | 选择子 + 计数 + 表 | 选择子 0 拿 `+2088` 与载荷列表比较，直到命中前走嵌套 `0x33` 体 | generic | confirmed |
| `0x34` | actor-kind-pair gate | 选择子；0 带动作组与编号 | 比较当前实际原生动作组／编号 `+20/+21`，任一不同则扫描 else/end；DSL：`self.in_action(group:id)` | named | confirmed |
| `0x35` | rage-active condition | 选择子 | 0：`+2726` 怒态标志非零时执行正文，否则跳至 else/end；1：else；2：end | named | confirmed |
| `0x36` | near-target-3d-gate | 选择子 + `u8` | 选择子 0 用**三维**距离比较 actor `+172`/`+176`/`+180` 与参考点 `+2852`/`+2856`/`+2860`，阈值为 `n×100`（无体型下限）；不超过则进入正文。DSL：`self.near_target_3d(n)` | named | confirmed |
| `0x37` | global-flag gate | 选择子 | 选择子 0 仅在全局对象标志 `+44` 不含位 `0x200000` 时继续，否则走嵌套 `0x37` 体 | named | confirmed |
| `0x38` | area-context table gate | 选择子 | 按当前区域 `+2040` 及 `+2008` 上下文读取 `off_1186916C`，非零抑制正文；表及上下文的具体含义仍未确定 | generic | operation_confirmed |
| `0x39` | flash-active condition | 选择子 | 0：`+2914` 闪光计时器非零时执行正文，否则跳至 else/end；1：else；2：end | named | confirmed |
| `0x3A` | nonzero-selection gate | 选择子 | 选择子 0 仅在 `+3186` 非零时继续，否则走嵌套 `0x3A` 体 | named | confirmed |
| `0x3B` | null-pointer gate | 选择子 | 选择子 0 仅在 `+3172`（关联 actor 指针）为空时继续，否则走嵌套 `0x3B` 体 | named | operation_confirmed |
| `0x3C` | other-actor gate | 选择子 | 关联 actor 存在、其模式 `+2680==1` 且当前区域 `+2040` 与自身相同才抑制正文；此处比较区域，不是物种 | named | operation_confirmed |
| `0x3D` | saved-area-context equality | 选择子；0 带 u16 大端区域 ID | 适配参数区域 ID 后与保存的区域／路线上下文 ID `+2846` 比较，不等则扫描正文 | named | confirmed |
| `0x3E` | saved-area-context list gate | 选择子 + 计数／u16 大端区域 case | 逐个适配区域 ID 并与保存的区域／路线上下文 ID `+2846` 比较；含 case、else、end 标记 | named | confirmed |
| `0x3F` | start-area-path-script | u16 大端区域 ID、u8 探测高度 | 选择区域路径及入口步并绑定 kind 9，保存续行 `+2648` 后转入 root[12] 路径脚本；FFFF 可选最近路线；末参数乘 10 加到射线两端 y，不是搜索半径 | generic | confirmed |
| `0x40` | set_runtime_mode | `u8` | 读一字节，为 0 时以 0、否则以 1 调用 `0x108693E0` | named | confirmed |
| `0x41` | conditional_angle_update | 选择子 | 选择子 0 仅在物种 27/28/31 且多个原生状态谓词通过时计算数值并调用 `0x10858E00`；完整业务含义未定，非通用转向 | generic | operation_confirmed |
| `0x42` | angle_threshold_branch | 选择子 + `u8` | 选择子 0 刷新向量，由后一字节得阈值，与 `0x10855830` 的绝对角差比较，按结果跨过 `0x42` 标记块 | generic | operation_confirmed |
| `0x44` | conditional_runtime_relation | 选择子 | 选择子 0 仅在 `+3172` 为空或其 `+2040` 与当前 `+2040` 不同时进入体；选择子 1 扫到终止符 | named | operation_confirmed |
| `0x45` | select_candidate_lane_by_flags | 选择子 | 选择子 0 扫四条全局记录（玩家域，`0x1DC6B750`，4176 步长），按活动掩码、当前 `+2040`、actor 类型 `+3`、记录标志过滤，按 `+2740` 浮点 lane 值排名，把选中下标写入 `+2612`（cmd_pl_target） | generic | operation_confirmed |
| `0x46` | conditional_lane_pair | 选择子 + 2 字节 | 选择子 0 读两字节对，与选中 lane 记录的 `+20`/`+21` 比较，不等则跨过 `0x46` 标记块 | named | confirmed |
| `0x47` | consume_runtime_flag_2729_branch | 选择子 | 选择子 0 读取 `+2729`，非零时清零并进入正文，否则扫描分支；是消费式条件，不是 wait | generic | operation_confirmed |
| `0x48` | set_ai_delay_frames | u8 帧数 | 写自身 AI 延迟 `+3228`；逐帧递减，为正时不执行解释器，归零后续行；DSL：`wait(n)` | named | confirmed |
| `0x49` | bind_selected_player_ground_point | u8 偏移档案 | 绑定当前玩家脚下编号对应的地面点及旋转偏移，kind=11；无目标时 kind=1、index=FFFF；同步保存的命令字段。已证档案 0..3，不是普通绑定玩家坐标 | named | confirmed |
| `0x4a` | lane_mask_gate | 选择子 | 目标非 FF 且 `+2684` 对应发现位已置时进入正文；不重做感知，也不验证目标域。DSL：`self.target_detected` | named | operation_confirmed |
| `0x4b` | clear_selected_player_awareness_score | 无 | 有选中玩家时清 `+2788 + 4*(slot & 0xF)` 的 awareness 累积分数，不清发现／追踪标志 | named | confirmed |
| `0x4c` | clear_selected_player_hate_score | 无 | 有选中玩家时清 `+2824 + 4*(slot & 0xF)` 的选敌仇恨分数；该字段不是计时器 | named | confirmed |
| `0x4d` | refresh_runtime_vectors | 无 | 按当前动作目标配置解析实体、位置或区域下一跳；DSL：`self.resolve_target()`，不执行移动 | named | confirmed |
| `0x4e` | extend_request_countdown_2696 | 选择子 | 选择子 0 给请求计时值 `+2696` 增加配置值 `+2708` 的一半并封顶；非零选择子不写；不是玩家仇恨分数 | generic | confirmed |
| `0x4f` | extend_request_countdown_2700 | 选择子 | 选择子 0 给请求计时值 `+2700` 补充 `+2712` 的一半并封顶；物种 104/112 还遍历活动同种 em 做同样补充并清其事件位 0x02；非零选择子不写 | generic | confirmed |
| `0x50` | extend_request_countdown_2704 | 选择子 | 选择子 0 给请求计时值 `+2704` 增加配置值 `+2716` 的一半并封顶；非零选择子不写 | generic | confirmed |
| `0x51` | runtime_phase_gate | 选择子 | 选择子 0 在 `+1040` 等于 4 时跳过体，否则扫 `0x51` 标记块 | named | confirmed |
| `0x52` | select_player_same_or_allowed_area | 无 | 从已追踪玩家中按原生优先级选敌，可接受同区域及物种配置允许的其他区域；保留原生目标保留规则。DSL：`EntityTarget::SameOrAllowedArea` | named | operation_confirmed |
| `0x53` | select_player_same_area | 无 | 同区域原生复合选敌，综合仇恨分数、距离、当前随机值及物种专用规则；DSL：`EntityTarget::SameArea` | generic | operation_confirmed |
| `0x54` | lane_record_gate | 选择子 | 选择子 0 在选中 lane 记录有效、其 `+2042` 为零且 `0x10A94CE0` 返回非零时直接返回，否则扫 `0x54` 标记块 | named | confirmed |
| `0x55` | selected_player_hate_threshold_branch | 选择子 | 选择子 0 在无选中玩家或该槽选敌仇恨分数 `+2824` 小于 30000 时跳过正文；不是计时器门 | named | confirmed |
| `0x56` | global_word_gate | 选择子 + 大端 `u16` | 选择子 0 读大端 `u16`，与全局 `0x1E8001EC`+8 的字不等时扫 `0x56` 标记块 | generic | operation_confirmed |
| `0x57` | area_route_profile_selection | 选择子 + 载荷 | 按 `u8 +3185` 区域移动路线配置编号有序精确分支；`00 count` 开始，`01 value:u16be` case，`02` else，`03` 结束。字段来自生成记录 `+0x18`，用于选择物种/地图的区域路线表 | named | confirmed |
| `0x58` | bind_leader_player_target | 无 | 复制关联首领的已提交玩家目标到 `+2612` 及动作目标参数，不同步 saved 命令字段；无首领则设无目标。DSL：`EntityTarget::LeaderTarget` | named | operation_confirmed |
| `0x59` | runtime_flag_gate_3200 | 选择子 | 选择子 0 仅在 `+3200` 为零时扫 `0x59` 标记块 | generic | operation_confirmed |
| `0x5a` | lane_value_gate | `u8` | 选择子 0 在保存的动作类型 `+3231` 为 11 或 1 且选中 lane 时，若值等于 `0x1086A530`(lane`+2040`, lane`+2008`) 则立即返回 | named | confirmed |
| `0x5b` | clear_undetected_player_tracking_timers | 选择子 | 选择子 0 清未设置发现位 `+2684` 的玩家之追踪保持计时 `+2688`，不立即清 `+2687`；非零选择子不写 | named | confirmed |
| `0x5c` | world_position_gate | 选择子 | 选择子 0 在 `+2040` 等于 `global+20` 且 `0x108CF5B0`(`+172`, …) 成功时立即返回，否则扫 `0x5c` 标记块 | named | confirmed |
| `0x5d` | target-reference-point-gate | 选择子 | 选择子 0 先解析一次当前目标，参考点 `+2852`/`+2856`/`+2860` 三个分量都非零时进入正文，否则扫到 `5D 01`/`5D 02`；分量等于 `0` 也算无效。DSL：`self.target_position_available()` | named | confirmed |
| `0x5e` | species_value_gate | `u8` | 选择子 0 拿载荷字节与 `0x1086A530`(`+2040`, `+2008`) 比较，并对表值 365/367/369 有特殊处理 | generic | operation_confirmed |
| `0x5f` | select_player_same_area_ground_group | 无 | 同区域并按自身地面编号分组过滤玩家，再按原生仇恨分数／距离规则选敌；DSL：`EntityTarget::SameAreaGroundGroup` | generic | operation_confirmed |
| `0x60` | global_flag_gate_9466 | 选择子 | 选择子 0 仅在全局字节 `0x1E7FFF3C`+9466 为 0 时扫 `0x60` 标记块 | generic | operation_confirmed |
| `0x61` | reload_attack_mode_timer | 无 | 从物种表按当前区域取值，重装攻击模式计时器 `+2912`；汇编 `0x10869DE8` 明确写 AX，不等待、不切模式 | named | confirmed |
| `0x62` | conditional_marker_98_gate | 选择子 + 2 字节 | 选择子 0 跳过两字节载荷，按辅助函数得出的选择子反复扫 `0x62`/`0x98` 标记；选择子 1..3 扫到 `0x62` 终止符 | named | confirmed |
| `0x63` | runtime_bitmask_gate | `u8` | 选择子 0 读掩码字节，(掩码 & `+27`) 为 0 时扫 `0x63` 标记块 | generic | operation_confirmed |
| `0x64` | selected_player_hate_table_threshold_branch | 选择子；0 带 u8 索引 | 无选中玩家或该槽选敌仇恨分数 `+2824` 低于 `0x11A4BFB0[index]` 时扫描分支；不是计时器门 | named | operation_confirmed |
| `0x65` | clear_player_hate_scores | 无 | 清四个玩家槽的选敌仇恨分数 `+2824/+2828/+2832/+2836`，保留发现位、追踪位、保持计时及 awareness 分数 | named | confirmed |
| `0x66` | all_records_empty_gate | 选择子 | 选择子 0 仅在全局记录集全部为空时扫 `0x66` 标记块 | named | confirmed |
| `0x67` | runtime_flag_2739_gate | 选择子 | 选择子 0 在 `+2739`（命令模式标志）为 0 时立即返回，否则扫 `0x67` 标记块 | named | confirmed |
| `0x68` | request_runtime_refresh | 无 | 当 `+2739`（命令模式标志）与 `+2659` 都为 0 时，置 `+2659`=2、`+2622`=1 并调用 `0x10860430` | named | confirmed |
| `0x69` | short_angle_threshold_branch | `u8` | 选择子 0 刷新向量，由后一字节缩放得阈值，与绝对角差比较后走 `0x69` 标记分支；另要求阈值小于 `0x4000` | generic | operation_confirmed |
| `0x70` | species_match | 选择子 + 计数 / u8 case | 按递增 case 精确匹配 `actor+3` 物种 ID，不做物种组归一化；DSL 为 `match self.species`，else 可省略 | named | confirmed |
| `0x71` | threshold_selector_gate | 选择子 + `u8` | 选择子 0 把载荷字节与带符号 `+3212`（带符号阈值）比较，不满足则扫嵌套 `0x71` 块 | named | confirmed |
| `0x72` | global_guard_selector_gate | 选择子 | 选择子 0 由全局 `0x1E8001EC`+8、`0x1ED52870`、`0x1ED52951` 门控后扫嵌套 `0x72` 块 | generic | operation_confirmed |
| `0x73` | masked_runtime_selector_scan | 选择子 + 计数 | 选择子 0 把列表与 `+2920`（目标状态字节）按 `0x7f` 掩码后比较，相等时清掉保留高位的字段 | named | confirmed |
| `0x74` | masked_runtime_threshold_gate | 选择子 + `u8` | 选择子 0 把载荷字节与 `+2920`（目标状态字节）按 `0x7f` 掩码后比较；受保护的辅助函数可让条件直接成立 | generic | operation_confirmed |
| `0x75` | ordered_runtime_2930_scan | 选择子 + 计数 | 选择子 0 把列表字节与 `+2930`（`0x75` 比较字节）比较，跳过或扫过嵌套 `0x75` 块 | generic | operation_confirmed |
| `0x76` | global_flag_ordered_scan | 选择子 + 计数 | 选择子 0 由全局 `0x1E8001EC`+2357 的位推出比较字节，比较列表后扫嵌套 `0x76` 块 | generic | operation_confirmed |
| `0x77` | daytime_branch | 选择子 | 全局 `0x1ED6BCA0` 昼位 `0x08` 已设置或夜位 `0x10` 未设置时进入正文；仅夜位设置时进入 else；两位均未设置也进入正文；DSL 为 `context.is_daytime` | named | confirmed |
| `0x78` | angle_interval_gate | 选择子 + 2 字节 | 选择子 0 读上下界字节，由 `+2068`（角度来源）与 `+164` 算当前角，条件成立才扫嵌套 `0x78` 块 | named | confirmed |
| `0x79` | callback_result_ordered_scan | 选择子 + 计数 + 参数 + 列表 | 选择子 0 用载荷字节 2 调 `+1140`（行为对象指针）+12 的函数指针，把返回字节与从偏移 4 开始的有序表比较 | named | confirmed |
| `0x7a` | mapped_global_ordered_scan | 选择子 + 计数 | 选择子 0 把全局 `0x1E7FFF3C`+52 经表 `0x118648F0` 映射后比较列表，再扫嵌套 `0x7a` 块 | generic | operation_confirmed |
| `0x7b` | increment_runtime_1096 | 无 | 递增 `+1096`（脚本 RNG 计数），只消耗 opcode 字节 | named | confirmed |
| `0x7c` | mapped_global_gate | 选择子 | 选择子 0 把全局 `0x1E7FFF3C`+52 经 `0x118648F0` 映射，映射值为 9/10/53/54 时直接继续，否则扫嵌套 `0x7c` 块 | generic | operation_confirmed |
| `0x7d` | actor_type_list_gate | 选择子 + 计数 + 列表 | 选择子 0 读计数与候选字节，用 `0x1087CB30`(`+3`, 候选) 逐个测试，未命中前跳过嵌套 `0x7d` 块 | named | confirmed |
| `0x7e` | candidate_lane_selection | 无 | 按原生配置先尝试玩家或怪物，失败后尝试另一类，绑定目标下标及目标类型；怪物域为最多 40 个 em，含同区、范围等资格检查。DSL：`EntityTarget::PlayerOrMonster` | generic | operation_confirmed |
| `0x7f` | bit_40_clear_gate | 选择子 | 选择子 0 的原生判定失败路径及选择子 1 的 else 扫描路径会清 `+1042` 位 0x40；不是纯只读条件 | named | confirmed |
| `0x80` | bounded_nested_delta_scan | 选择子 + 计数 | 选择子 0 读计数，扫嵌套 `0x80` 记录，把每条第 3 字节的带符号增量按 `+1096` 与 31 偏移累加 | named | confirmed |
| `0x81` | call_primary_subscript | `u8` | 调用 `root[1][index]`；从 stage 0 调用时保存 `0x81` 后续行游标到 `+2552`，同层调用则是尾跳转；置 stage=1 | named | confirmed |
| `0x82` | call_secondary_subscript | 2×`u8` | 调用 `root[15 + group][index]`；从 stage 1 调用时保存 `0x82` 后续行游标到 `+2556`，同层调用则是尾跳转；置 stage=2 | named | confirmed |
| `0x83` | lane_value_ordered_skip | 选择子 + 计数 | 选择子 0 把计数列表与选中 lane 值比较（`+3231` 为 13 时改用距离推导值），把值写 `+1112`，并把跳过计数存 `+2586` | generic | operation_confirmed |
| `0x84` | increment_runtime_1096 | 无 | 与 `0x7b` 共用内联递增路径：递增 `+1096`（脚本 RNG 计数） | named | confirmed |
| `0x85` | set_runtime_flag_40 | 无 | 置 `+1042` 位 0x40 并调用 `0x113AAAC0`，后者依赖目标对象坐标进行空间处理；位的游戏语义未定，不是对称 bool setter | named | operation_confirmed |
| `0x86` | clear_runtime_flag_40 | 无 | 仅清 `+1042` 位 0x40，不执行 85 的额外调用 | named | confirmed |
| `0x90` | place_at_species_point | u8 物种点索引 | 从 `0x118C5784[10*species]` 物种固定点表取三维坐标，写自身位置并同步上一位置快照；原生不查空表或索引范围 | named | operation_confirmed |
| `0x91` | face_species_point | u8 物种点索引 | 索引与 90 同一物种点表，计算从自身当前位置到该点的平面朝向并写 `+164`；参数参与计算，不读取当前目标点 | named | confirmed |
| `0x92` | consume_only | 无 | 除消耗 opcode 字节外没有观察到其他动作 | named | confirmed |
| `0x93` | consume_only | 无 | 与 `0x92` 相同的空实现 | named | confirmed |
| `0x94` | global_value_ordered_scan | 选择子；0 带计数，1 带 u8 case | 有序精确匹配全局 i32 `0x11C6A7E8`；更大的 case 不会命中，而是跳至默认分支或结束。原生脚本在常规 AI 与 root[18][4] 之间选择；非零值来源未确认 | generic | operation_confirmed |
| `0x99` | set_heading_u8 | u8 角度刻度 | 将参数左移 8 位写到 dword `+164`，直接设置绝对朝向；256 刻度一圈，不等待渐进转身 | named | confirmed |
| `0x9a` | scaled_runtime_threshold_gate | 选择子 + `u8` | 选择子 0 把载荷字节按全局浮点 `0x119B5EF0` 与 `+2712` 缩放，与 `+2700` 比较后条件性跳过嵌套 `0x9a` 块 | generic | operation_confirmed |
| `0x9b` | selected_record_gate | 选择子 | 选择子 0 用原始记录标志、辅助函数与记录偏移 `+1040` 检查 `+1826`（玩家域下标，解析到 `0x1DC6B750 + 4176*slot`）处的选中记录，全部通过则立即返回 | named | confirmed |
| `0x9c` | action_context_distance_gate | 选择子 + `u8` | 选择子 0 把载荷字节放大 100 倍，仅当动作字段 `+2581`/`+2582`/`+2584` 与 `+2856`/`+1800` 距离满足观察到的谓词时才穿过嵌套体 | named | operation_confirmed |
| `0xff` | control_prefix | `u8` 选择子 | 控制前缀：后一字节是选择子；`0x00..0x06` 与 `0xf5..0xff` 是命令，其余选择子交回解释器按普通 opcode 执行（此时 `0xff` 只消耗自身） | named | confirmed |

## 4. `0xff` 控制命令（选择子）

| 选择子 | 名称 | 语义 |
| --- | --- | --- |
| `0x00` | end | 重装主表第 0 项，清活动 lane 掩码并标记脚本停止：`+2576`=0、stage `+2580`=0、游标 `+2652`=main[0]、`+3288`=0、`+2659`=2 |
| `0x01` | contents_return | 回到保存的 contents 游标，并把选择 stage 切回 0（contents）；原生日志 `CONTENTS TBL END` |
| `0x02` | sub_contents_return | 回到保存的 sub-contents 游标，把 stage 切到 1；原生日志 `SUB CONTENTS TBL END` |
| `0x03` | route_return | 回到第三个保存游标 `+2608` |
| `0x04` | ground_area_move | 共享地面区域移动步进状态机；可推进路径、恢复续行，保存 kind=9 时在完成路径应用区域／位置落点 |
| `0x05` | ground_area_move | 与 04 共用步进入口，但不执行仅 selector=4 且保存 kind=9 时的区域／位置落点；不能按完全同义编码处理 |
| `0x06` | ground_area_move_snap | 保存 kind=9 时直接进入区域／位置步进放置路径，其余仍走原生路径状态机；不是无条件传送 |
| `0xf5` | unko_end | 硬重置到第一条主表项：`+2576`=0、stage=0、游标=main[0][0]、`+3288`=0、`+2659`=2；原生日志 `UNKO_END` |
| `0xf6` | no_floor_end | 与 `0x00` 相同的重置，用于路由没有地面时；原生日志 `NO_FLOOR_END` |
| `0xf7` | find_ng_end | 重置主选择并清当前目标槽的追踪位、保持计时、awareness 和选敌仇恨分数；无目标时不清，且不直接把 +2612 写成 FF |
| `0xf8` | clear_lane8 | 清 lane 位 `0x0008`；若 lane 位 `0x0001` 仍在则恢复 `main[2][+2595]`，否则走完整重置 |
| `0xf9` | clear_lane4 | 清 lane 位 `0x0004`；同上按 `0x0001` 决定恢复还是重置 |
| `0xfa` | clear_lane2 | 重装主选择，清 lane 位 `0x0002` 并调用 `0x1084FB80`(em, 0)；再按 `0x0001` 决定恢复 `main[2][+2595]` 或重置 |
| `0xfb` | area_end | 恢复 +2616 并清 +2621；有符号比较列表数量 +2844 与已选下标 +2845，数量较大则直接续行，否则按 +2620 清两者为 FF 或将已选下标复位为 0 |
| `0xfc` | find_end | 命令模式 +2739 为 0 时先调用 Em_Mode_Chg(em,1)，再将实际模式 +2680 保存到 +2600；随后重置主选择及 lane 字，不写 +2681 |
| `0xfd` | kehai_end | 结束気配路由：清 lane 位 `0x0010`；lane 位 `0x0001` 仍在时按 `+2595` 的 route act 恢复 `main[2][+2595]`（`route_ptr_set`），否则重装主项并置网络命令状态（`em_cmd_top` + `EM_NET_CMD`） |
| `0xfe` | route_move_end | 解析参考点并按物种半径判断到达；到达后增加 +2842、清 +3250。模式 0 用 +1096 从候选中排除当前点选择，1 读 root[2][+2593][进度]，2 递增航点；恢复 root[2][+2595] |
| `0xff` | loop_count | 路线进度 +2842 未达上限 +2841 时恢复 root[2][+2594]；达到上限则清基 lane 位 0x0001，并由 lane 选择器决定下一游标 |
| 其他 | re-dispatch | 未出现在两张 switch 表（`0x108675D4` 覆盖 `0x00..0x06`、`0x10867CDC` 覆盖 `0xf6..0xff`）里的选择子，交回解释器按普通 opcode 执行 |

注意 `0xff` 家族里 `0x04`/`0x05` 共用步进入口，但完成路径检查选择子，二者的
区域／位置落点行为不同，不能按同义编码归一化。`0x01`/`0x02`/`0x03` 是三级
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

## 6. 按用途分类

以下列出代表指令，类别可以重叠；不以连续 opcode 范围代替逐条分类。

| 用途 | opcode 示例 |
| --- | --- |
| 游标转移、动作分派与调用 | `04/05/07/0F/16/18/3F/81/82`、`FF` 家族 |
| 原生循环及计数 | `24/25`、`FF FF`；`25` 只清计数，不跳出正文 |
| 条件与多分支 | `02/08/09/0E/15/1D/22/28/29/2A/2C/35/36/39/4A/54/57/70/77/78/79/80/83/94` |
| 目标配置与绑定 | `06/10/11/12/13/19/1A/2D/31/49/52/53/58/5F/7E` |
| 感知、追踪与请求簿记 | `1E/2E/30/4B/4C/4E/4F/50/5B/61/65` |
| 位置解析、放置与朝向 | `4D/90/91/99`；`4D` 解析目标，`90` 直接改位置 |
| 字段写入与状态控制 | `0A/0C/0D/17/26/40/48/85/86` |
| 空操作 | `92/93` |

## 7. 控制指令的 DSL 覆盖与建议

本节按 2026-09-26 的编译器、反编译器和安装校验核对。控制范围包括非条件的
动作、状态写入、目标配置、游标控制以及 `24` 循环；纯条件／多分支不计入。
`native(...)` 转义不等于具名语义支持，`domain: named` 也不保证已有可用 DSL。
下表中的建议均**尚未实现**，不能直接复制为当前可编译语法。

| 范围 | 已有具名表达 | 部分接入 | 尚无具名表达 |
| --- | ---: | ---: | ---: |
| 普通控制 opcode，56 个 | 25 | 3（`06/0C/0D`） | 28 |
| `FF` 有效选择子，18 个 | 14 | — | 4（`04/05/06/FF`） |

25 个已有表达的普通 opcode 是
`04/05/07/11/12/13/16/18/1E/2D/2E/40/48/4D/52/53/58/5F/68/7B/7E/81/82/84/92`。
其中 `2E` 已有 `self.select_perception_profile(n)`；`7B/84` 已有
`self.increment_random_value()`，编译统一生成 `84`。
`07 00`、`40` 的非标准非零参数等为保真而保留 native，不代表缺少新的行为。

### 7.1 尚无具名表达的 28 个普通控制 opcode

| opcode | 原生行为与接入边界 | 推荐形式／决定 |
| --- | --- | --- |
| `0A` | 写 `+2088`，影响模式转换、感知和异常状态；完整取值语义未定，不是 `+2680` 的 Mode | 保留 native，不命名为 `set_mode`、sleep 或 stun |
| `0F` | 建立路线移动上下文，保存续行并转入 `root[2]`；不是单纯配置字段 | 后续与路线资源成套设计 `self.start_route_move(...)` |
| `10` | 绑定当前路线点，仅在点无效／越界时重选，另增加 `+3250` | 暂定 `self.bind_route_waypoint()`，不能叫 `next_waypoint()` |
| `17` | 仅在 `+2844 == FF` 时初始化 `root[5]` 区域列表及 `root[6]` 处理脚本等上下文；`18` 才尝试换区 | 暂定 `self.init_area_route(...)`，保留已有上下文时不重配的行为 |
| `19` | 将已经选出的下一地区 `+3208` 绑定为区域动作目标，不换区 | 优先候选 `self.bind_next_area()` |
| `1A` | 适配区域 ID，同时写 `+2046/+3208` 及区域动作目标；不写当前区域、不位移 | 优先候选 `self.set_next_area(area)`；区别于会清目的地缓存的 `select_target_area(area)` |
| `24` | 唯一的 i8 重复计数与体游标，执行／扫描宽度不一致 | 暂不接入；未来 `repeat(n) { ... }` 须先解决边界、嵌套／重入及 `n <= 0` 路径，不能把 128～255 当正次数 |
| `25` | 只清重复计数，继续执行剩余正文；后续 `24 01` 再减计数 | 将来可用 `stop_repeating()`，不能直接映射为 `break` |
| `26` | 写 `+2738`，消费者分别检查 `==0` 和 `!=1`，不是完整 u8 域上的布尔开关 | 保留 native，待自动响应控制的取值域明确 |
| `30` | 有配置玩家搬运时置优先级 `0x70` 的待处理请求，不选目标、不保证立即执行 | 候选 `self.request_carry_response()` |
| `31` | 绑定 kind 8，后续解析使用物件扫描缓存坐标；不是出生点／归巢点 | 候选 `self.bind_cached_object_point()` |
| `3F` | 选择区域路径、保存续行并进入 `root[12]`；末参数乘 10 是探测高度偏移，不是半径 | 暂定 `self.start_area_path(area, probe_height)`，与 `FF 04/05/06` 成套设计 |
| `41` | 特定物种及状态下计算数值并调用专用更新函数 | 保留 native，不视作通用转向 |
| `49` | 绑定当前玩家脚下编号所选的地面点及旋转偏移，同步保存的命令上下文；无目标时绑定无效玩家目标 | 优先候选 `self.bind_target_ground_point(profile)`，先限 `0..3`，不猜测前后方／攻击用途枚举 |
| `4B` | 清当前目标玩家的 awareness 累积分数；无目标不写 | 候选 `self.clear_target_awareness_score()` |
| `4C` | 清当前目标玩家的选敌仇恨分数；无目标不写 | 候选 `self.clear_target_hate_score()` |
| `4E/4F/50` | 三类请求计时值补充各自配置值的一半并封顶；`4F` 对物种 104/112 还传播到活动同种怪物并清相应事件位 | 暂留 native，待请求业务名与联动契约明确；不能叫 add_hate、heal 或纯自身 setter |
| `5B 00` | 清未被发现玩家的追踪保持计时，不立即清追踪位 | 优先候选 `self.clear_undetected_player_tracking_timers()`，不能叫 forget players |
| `61` | 按物种及当前区域的配置重装攻击模式计时器 `+2912`，不等待、不切模式 | 优先候选 `self.reload_attack_timer()` |
| `65` | 清四个玩家槽的选敌仇恨分数，保留其他感知／追踪数据 | 候选 `self.clear_player_hate_scores()`，不是清全部实体的全部仇恨状态 |
| `85/86` | 置／清状态位 `0x40`；只有 `85` 另有依赖目标对象的空间处理调用 | 保留 native，不能假设为对称 bool setter 或碰撞开关 |
| `90` | 从物种固定点表直接设置位置并同步上一位置快照；原生无索引检查 | 候选 `self.place_at_species_point(index)`，注明物种表范围 |
| `91` | 面向同一物种点表中的点，直接修改绝对朝向；参数参与点表索引 | 候选 `self.face_species_point(index)` |
| `93` | 与 `92` 同行为的空操作，现有 `nop()` 生成 `92` | 继续 native，无需新 API；不能未经明确规则就归一化字节 |
| `99` | 写 `u8 << 8` 到朝向，不移动、不等待渐进转身 | 候选 `self.set_heading(degrees)`，沿用度数 API 并明确 1.40625° 量化及往返规则 |

`49` 的四档局部偏移为 `(0,500,1000)`、`(0,400,800)`、`(0,400,1500)`、
`(0,600,600)`，旋转后加到玩家脚下编号对应的地面配置点上，不是直接偏移玩家坐标。
`90/91` 的物种固定点表也不同于现有 `select_target_point(index)` 的当前区域路线点表。

### 7.2 部分接入与原始编码边界

| 家族 | 已接入 | 仍未接入与推荐方向 |
| --- | --- | --- |
| `06` | mode 1 玩家槽；mode 2 Default／路线点；mode 3 区域；mode 6 八个固定方向；mode 10 目标玩家区域；mode 13 subtype 0..3 | mode 2 的其他点表来源及 mode 5/7/8/9/11 等应按目标用途分别接入。可先考虑物种点 `select_species_point(index)`；原始上下文 mode 0/4/12、未知 mode 和非规范占位参数仍 native |
| `0C/0D` selector 1 | 无独立写法 | 感知／闪光抑制计数；候选 `set_perception_suppression(n)` 及独立清除方法，说明逐帧递减与闪光联动 |
| `0C/0D` selector 2 | 无独立写法 | `+2696` 计时请求的一次性已触发锁存 `+2736`，不是计时值；待请求业务名确定再提供 rearm/suppress |
| `0C/0D` selector 3 | 无独立写法 | 感知更新抑制计数，Attack 模式先清零，也门控缓存物件点扫描；候选 `set_awareness_delay(n)`，不是暂停整个 AI |
| `0C/0D` selector 4 | `handle` 内部发 `0C 04 01`；`pass` 发 `0D 04` 加本函数返回 | 单纯清接管后继续仍 native；可另加 `clear_takeover()`，不能把裸 `0D 04` 恢复为 `pass` |
| `0C/0D` selector 5..9 | 无 | 物种私有字段保持 native |
| selector 0、`0D 10` | 无 | 前者批量操作异质字段，后者清保存的区域／路线上下文 ID；继续 native |

`0C/0D` 是整字节写入／清零，不是通用按位 flags API。
`0C selector,0` 与 `0D selector` 即使部分效果相同，也要分别保留原始编码；
不能默认通过 setter(0) 抹去差异。kind 8 的缓存点来自 `0x1ECB1680` 扫描表，
与已支持的 `2D` 所用 `0x1ED8BE80` 物件表分开记录。

### 7.3 FF 的接入边界

| 子指令 | 当前状态／原生行为 | 推荐方向 |
| --- | --- | --- |
| `00`、`F7` | 已有 `end`、`end forget_target` | 不计为缺口 |
| `01/02/03` | 由函数槽位的 `return`／收尾生成 | 不增加不受作用域约束的通用 return 别名 |
| `F5/F6/F8/F9/FA/FC/FD` | 七个事件的自动收尾及对应作用域 `return` | 已接入；不匹配当前作用域的原始尾仍 native |
| `FB/FE` | 已有 `area_end()`、`route_move_end()` | 不计为缺口 |
| `04` | 地面区域移动步进；保存 kind=9 时在完成路径应用区域／位置落点 | 路线上下文中的 `ground_move_step_end()`，保留落点条件 |
| `05` | 共用 04 的步进入口，但跳过仅 selector=4 才执行的区域／位置落点 | 单独保留此收尾变体，不能归一化成 04 或共用不带区别的语法 |
| `06` | 保存 kind=9 时可直接进入步进放置路径，其余仍走路径状态机 | 独立路径步放置／收尾形式，不能无条件称 teleport |
| `FF` | 未达路线循环上限则回到路线脚本，否则清基 lane 并重新选择游标 | 路线上下文中的 `route_loop_end()`，不是普通 return/repeat |

顶层 `F6..FE` 属于 default stop；只有带 `FF` 前缀时才是上述控制命令。
未知 FF 选择子 `07..F4` 原生会重新分派，当前安装校验拒绝，不应视为普通二字节命令。

### 7.4 带副作用的条件与验证边界

以下仍属于分支结构，未计入 28 个普通控制 opcode：

| opcode | 分支副作用 | 推荐方向 |
| --- | --- | --- |
| `03` | 区域目标接受检查可能重置动作上下文 | 有副作用的条件方法，不作为纯属性 |
| `45` | 筛选玩家、写 `+2612` 并按成败分支 | 将来 `if self.try_select_...() { ... }`，筛选业务名未定前 native |
| `47` | 读取并消费 `+2729` 一次性标志 | 将来消费式条件方法，不是 wait |
| `73` | 匹配 `+2920 & 0x7F` 后清低位、保留高位 | 消费式 match，不能省略清除副作用 |
| `7F` | 条件失败路径及 else 标记会清状态位 `0x40` | 保留 native，不拆成会改变求值顺序的纯 if 加 setter |
| `62` | 混合 `62/98` 扫描且执行／跳过宽度不一致 | 与 `24` 一样，当前包括 native 在内均拒绝安装 |

优先顺序建议为 `49/5B/19/1A/61` 与分数清理，然后是缓存点及位置／朝向操作；
路线家族成套设计，循环与物种私有控制后置。这是设计建议，不是已承诺的实现。

覆盖核对依据：`src/ai/dsl/compile.rs` 的命令与语句编码、`dsl/target.rs`、
`dsl/slot.rs`、`src/ai/decompile.rs` 的语义恢复、`src/ai/control.rs` 的事件尾映射，
以及 `src/ai/bytecode.rs` 的结构校验。
原生证据详见各 JSON 记录的 `evidence` 和 [`runtime-fields.md`](runtime-fields.md)：
`0x108611F0/0x108612F0`（字段写入）、`0x10861530/0x10861660/0x10861D30`
（路线）、`0x10863B20`（路径与探测高度）、`0x10864640/0x108538B0`（49）、
`0x108535C0/0x108552C0/0x10852A80/0x10852C10/0x10854C70`（分数与保持计时）、
`0x10869DE8/0x1085C170`（61）、`0x10868210/0x10868270`（物种点）、
`0x10864790`（4F 联动）。本轮结论为静态证据，未作游戏运行验证。

## 8. 仍然存在的缺口

数据角色的剩余数量见第 1 节，逐项清单以 JSON 的 `domain_unresolved` 和
[`runtime-fields.md`](runtime-fields.md) 为准。字段写入已确认不代表私有值的游戏含义
已确认，尤其是物种私有控制、三类计时请求的业务名及状态位 `0x40`。
原生字节识别、语义 DSL 覆盖和安装边界是三件不同的事；第 7 节分别记录，
不能从 256 字节都有记录推导出全部可独立安装或全部可具名编写。
