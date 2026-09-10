# MHF translation dictionaries

翻译源文件是无 BOM 的 UTF-8 JSON Lines。每个文件独立定义一个 locale，文件可以为空。
`mhf-translation/provider` 负责词典生成与运行时查询；默认 API 与配置层不读取字典。
Unicode provider 可独立转码和绘制原文。二者共用
[Unicode 资源布局](../../unicode/resources/layout.json) 的稳定字符串 ID，但词典 ordinal
只属于 Translation 实现。省略 Translation provider 时，宿主保留配置且原文照常转码。
文件名就是 locale ID，不要求采用特定语言代码格式；例如 `zh-CN.jsonl` 对应：

```toml
[translation]
locale = "zh-CN"
missing = "original"
```

资源 schema、运行时加载边界和转码规则见 [资源布局说明](../../unicode/resources/README.md)。

## Keys

`records` 资源使用“资源、稳定表 ID、表内零基索引”；INF 使用记录内的稳定 quest ID：

```text
mhfdat:head_armor_names:0
mhfdat:item_names:15032
mhfinf:quest:25001
mhfpac:table_026:0
mhfjmp:destinations:0
mhfgao:skill_names:1
mhfsqd:special_effects:0
mhfrcc:events:0
mhfmsx:treasure_colors:0
```

所有数值组件都按数值解析，可以添加任意数量的前导 `0`；例如
`mhfdat:head_armor_names:12` 与 `mhfdat:head_armor_names:000012` 是同一个 key，不能同时定义。
普通记录索引必须适合 `u32`，quest ID 必须是非零 `u16`。新增或移动其他表、调整 quest
分类顺序都不会改变既有 key。

Stage TLK 使用：

```text
stage:125:001C:0000
```

stage 编号来自实际加载的 `stage[-hd]/stNNN.pac`，范围为 `0..=999`；section 和 record
按 `u16` 十六进制解析，也可以带 `0x` 前缀。生成的缺失标记会把 stage 规范化为三位、
section/record 规范化为四位大写十六进制。TLK 记录按确认结果直接加入 locale 文件。

## Equipment and item keys

DAT 中已确认用途的名称和描述使用业务 ID：

| 原 group | 当前 group | 索引含义 |
| --- | --- | --- |
| `table_000` | `head_armor_names` | 头部防具 ID |
| `table_001` | `body_armor_names` | 身体防具 ID |
| `table_002` | `arm_armor_names` | 手臂防具 ID |
| `table_003` | `waist_armor_names` | 腰部防具 ID |
| `table_004` | `leg_armor_names` | 腿部防具 ID |
| `table_005` | `armor_descriptions` | 五部位共用说明表索引，三行说明 |
| `table_006` | `ranged_weapon_names` | 远程武器 ID（轻弩、重弩、弓） |
| `table_007` | `melee_weapon_names` | 近战武器 ID |
| `table_008` | `melee_weapon_descriptions` | 同一近战武器 ID，三行说明 |
| `table_009` | `ranged_weapon_descriptions` | 同一远程武器 ID，三行说明 |
| `table_010` | `item_names` | 物品 ID |
| `table_011:0..23` | `item_messages:0..23` | 物品操作提示序号 |
| `table_011:24..16724` | `item_descriptions:0..16700` | 物品 ID，旧索引减去 24 |
| `table_016` | `item_acquisition_hints` | 同一物品 ID |

例如同一物品使用 `mhfdat:item_names:15032`、`mhfdat:item_descriptions:15032` 和
`mhfdat:item_acquisition_hints:15032`。物品描述只在布局中声明 `first_record: 24`，
不修改游戏中的物理表和原生下标。外部旧词典按上表迁移 key，原文和译文保持原值。
防具说明沿用共享表索引；当前头／身／腕／腰／脚说明的起点分别为
`0 / 14594 / 28056 / 41508 / 55216`，不应将它误当作部位内装备 ID。

## Dictionary rows

单字符串记录可以直接写字符串：

```json
{"key":"mhfpac:table_026:0","source":"~C05オプション~C00","translation":"~C05选项~C00","context":"选项菜单标题"}
```

多字符串记录使用长度与布局 `parts` 完全一致的数组；`null` 表示该 part 不提供覆盖：

```json
{"key":"mhfdat:armor_descriptions:42","source":["说明第一行","说明第二行",null],"translation":["第一行译文","第二行译文",null]}
```

- `key` 必填，并在构建期根据 layout 校验表 ID、固定记录表的索引范围、quest ID 类型和
  part 数量。
- `translation` 可以是字符串、数组、显式空字符串或 `null`。只有非 `null` 的翻译会
  编译进启动器内嵌的二进制字典。
- `source`、`context` 和 `note` 是翻译辅助信息，不会嵌入启动器。
- 每个 locale 都可以只保留已经翻译的稀疏行；空文件会编译成没有覆盖的 locale。

构建期由 layout 计算合法的主资源 key；运行时按相同 layout 遍历实际存在的非空字符串
槽位。`missing = "key"` 的标记和需要转码的原文保存在地址稳定的 arena 中。

JMP/GAO/SQD/RCC/MSX 的日文原文收录于 `ja-JP.jsonl`；其他 locale 使用相同 key
提供覆盖。接入解析不等于已经编写完整中文译文。


## Native text syntax

翻译中直接书写游戏原有语法。printf 占位符（如 `%s`、`%d`）、波浪号控制（如
`~A`、`~C02`、`~K123`）和花括号控制（如 `{I1}`、`{K2}`、`{u12}`）都按原始 ASCII
字节透传，不做严格语义检查。

JSON 转义完全由 JSON 解析器处理；解析后的 ASCII 字符会直接编译为同值字节，例如
`\n` 为 `0x0A`、`\u001A` 为 `0x1A`。若需要字面量反斜杠则写 `\\n`；`\u0000`
会生成 NUL，游戏会把它视为字符串结束符。

`build.rs` 为每条 UTF-8 译文追加 NUL，通过 `include_bytes!` 直接编入 EXE。运行时只查找
所选 locale 的静态记录，不分配虚拟字形编号，不复制译文，也不解析外部 JSON。
统一文字后端读取 UTF-8 并调用宽字符 GDI；游戏原有的 ASCII 格式控制保持原样。

## Generating a locale template

从已解密、已解压的资源 image 生成对应 locale 的翻译模板：

```text
python3 tools/generate_translation_dictionary.py \
  --locale zh-CN \
  --dat /path/to/mhfdat.bin \
  --inf /path/to/mhfinf.bin \
  --pac /path/to/mhfpac.bin \
  --jmp /path/to/mhfjmp.bin \
  --gao /path/to/mhfgao.bin \
  --sqd /path/to/mhfsqd.bin \
  --rcc /path/to/mhfrcc.bin \
  --msx /path/to/mhfmsx.bin
```

八个资源参数均可选，但至少提供一个；只刷新传入的资源，保留其他资源和已有译文。

`--locale zh-CN` 默认生成 `locales/zh-CN.jsonl`；也可以使用 `zh`、`chs` 或
`my_translation` 等自定义 ID。原始日文写入 `source`；目标文件中已有的翻译字段和未由
主资源生成的记录会保留，因此可以再次运行生成器刷新原文。通过 `--output-dir` 可以指定
其他输出目录。游戏资源只在执行生成器时需要；Unicode provider 的 `build.rs` 只需读取
`../unicode/resources/layout.json`；Translation provider 的构建才额外读取 `locales/*.jsonl`。

## Missing translations

- `original`：没有覆盖时，按明确的源代码页将原文转换为 UTF-8。
- `key`：未翻译非空槽位显示动态标记，例如
  `[mhfdat:head_armor_names:12]`；多 part 记录的后续 part 会追加 `:01`、`:02`。
- `empty`：把未翻译非空槽位替换为空字符串。

Stage TLK 按目录遍历全部物理 section；每个 section 的首字符串偏移就是指针表字节数，
除以 4 得到记录数。每条字符串必须在本 section 内以 NUL 结束。没有译文的记录同样转为
UTF-8，不依赖字典列出了哪些 key。若目录重复使用 section ID，只有首次匹配使用译文，
后续 section 仍转换原文，与游戏按首次匹配取表的行为一致。Stage 未识别时只转换原文。
普通 stage PAC 内 TLK 始终使用 CP932；`localize-{usa,kor,twn}-tlk.bin` 的覆盖入口按
原语言字段选择 CP932/949/950。两个调用点经过签名校验；语言包使用原函数传入的 stage
索引，避免将它和普通 PAC 的实际文件号混用。无效源字节会报告对应代码页错误，不替换字符。

翻译覆盖与游戏原生 `[localization] language` 相互独立。以日文资源制作的字典通常仍让
游戏语言保持 `japanese`，这样 `missing = "original"` 会回退到日文。
启用 Translation provider 但省略 `[translation]` 时，只关闭译文覆盖，资源仍转换为 UTF-8。
关闭 Translation provider 后，Unicode provider 仍可独立转换原文。只有关闭 Unicode Mod
时才停止资源和 DLL 文本转码，游戏保留原生文本处理。

### Native image text

The dictionary also accepts `native:rank:<0..4>`, `native:room:<0..5>` and
`native:literal:<decimal source RVA>`. Japanese sources are included in
`ja-JP.jsonl`; use the same keys in another locale. Native keys are validated
against `../unicode/src/provider/resources/native/layout.rs`, which is shared with the native
record reader. Rank records and room pointer fields are parsed by shape;
only scattered compiled operands use explicit bindings. No runtime scan or
encoding heuristic is used. The same missing-translation setting applies.
