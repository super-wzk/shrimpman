# MHF translation dictionaries

翻译源文件是无 BOM 的 UTF-8 JSON Lines。每个文件独立定义一个 locale，文件可以为空。
文件名就是 locale ID，不要求采用特定语言代码格式；例如 `zh-CN.jsonl` 对应：

```toml
[translation]
locale = "zh-CN"
missing = "original"
```

## Layout

`resources.json` 描述主资源的字符串结构及其在目标客户端中的运行时绑定。相同的
记录结构只在 `record_layouts` 中定义一次：

```json
"text_1_at_0_stride_4": {
  "stride": 4,
  "text_offset": 0,
  "parts": 1
}
```

- `stride` 是相邻记录起始位置的字节间隔，必须显式填写。
- `text_offset` 是记录起点到第一个字符串指针字段的字节偏移。
- `parts` 是每条逻辑记录包含的连续字符串指针数量。

每个资源用 `type` 明确选择一种结构。DAT/PAC 是 `records`，只包含普通表与连续表目录：

```json
{
  "id": "mhfdat",
  "identity": { "magic": 442919021, "format_version": 89 },
  "runtime": {
    "post_relocation": {
      "rva": "0x00AFA056",
      "signature": "8B 81 E8 00 00 00 0F B7 70 08 0F B7 48 04"
    },
    "buffer_rva": "0x0E77DCC4",
    "size_rva": "0x0EDB9B5C"
  },
  "type": "records",
  "tables": [
    { "id": "hunter_guide_sections", "root_field": 416, "records": 4, "layout": "text_1_at_0_stride_12" }
  ],
  "table_directories": []
}
```

`runtime.post_relocation` 指向资源完成内部指针重定位后的 hook 边界，`buffer_rva` 和
`size_rva` 指向模块中保存资源 image 地址与大小的字段。RVA 使用 `0x` 前缀的十六进制
字符串；signature 以空格分隔字节，`??` 表示随客户端构建变化的字节。构建脚本根据这些
字段生成资源绑定、签名、detour 和保存 trampoline 的槽位。

单独的表只引用记录布局：

```json
{
  "id": "title_menu",
  "root_field": 1528,
  "records": 4,
  "layout": "text_1_at_0_stride_4"
}
```

- `id` 是该表的稳定标识，也是翻译 key 的一部分。它在同一资源内必须唯一；新增表应
  分配新 ID，不能根据物理位置重新编号已有 ID。
- `root_field` 是资源 image 中保存记录表指针的字段位置。
- `records` 是记录数量，不能根据翻译字典的内容推断。

若多个相邻根字段指向相同形状的记录表，使用 `table_directories`：

```json
{
  "first_root_field": 1634304,
  "root_stride": 4,
  "layout": "text_1_at_0_stride_4",
  "entries": [
    { "id": "table_1013", "records": 4 },
    { "id": "table_1014", "records": 4 }
  ]
}
```

每个 entry 的根字段为 `first_root_field + entry_index * root_stride`。构建期会将目录展开
成普通表，因此它只压缩布局描述，不改变运行时遍历、翻译 key 或表内索引。

INF 使用独立的 `quest` 资源结构：

```json
{
  "id": "mhfinf",
  "identity": { "magic": 442920553, "format_version": 6 },
  "runtime": {
    "post_relocation": {
      "rva": "0x00AFB546",
      "signature": "8B 4D 08 C7 01 00 00 00 00 5F 5E 5B 8B E5 5D C3"
    },
    "buffer_rva": "0x0E77DCC8",
    "size_rva": "0x0EDB9B70"
  },
  "type": "quest",
  "layout": {
    "id": "quest",
    "root_field": 20,
    "count_root_field": 16,
    "category_stride": 8,
    "category_count_field": 2,
    "category_records_field": 4,
    "record_text_field": 40,
    "record_id_field": 46,
    "parts": 8
  }
}
```

`record_id_field` 指向 quest 记录自身的 `u16 questID`；启动器和导出器都直接读取该值，
该值就是 quest 翻译 key 的记录 ID。

表 ID 是翻译字典的稳定接口。未知业务含义使用中性 ID；已被字典引用的 ID 不应仅为
改善名称而更改。

## Keys

DAT/PAC 使用“资源、稳定表 ID、表内零基索引”；INF 使用记录内的稳定 quest ID：

```text
mhfdat:table_000:0
mhfinf:quest:25001
mhfpac:table_026:0
```

所有数值组件都按数值解析，可以添加任意数量的前导 `0`；例如
`mhfdat:table_000:12` 与 `mhfdat:table_000:000012` 是同一个 key，不能同时定义。
DAT/PAC 索引必须适合 `u32`，quest ID 必须是非零 `u16`。新增或移动其他表、调整 quest
分类顺序都不会改变既有 key。

Stage TLK 使用：

```text
stage:125:001C:0000
```

stage 编号来自实际加载的 `stage[-hd]/stNNN.pac`，范围为 `0..=999`；section 和 record
按 `u16` 十六进制解析，也可以带 `0x` 前缀。生成的缺失标记会把 stage 规范化为三位、
section/record 规范化为四位大写十六进制。TLK 记录按确认结果直接加入 locale 文件。

## Dictionary rows

单字符串记录可以直接写字符串：

```json
{"key":"mhfpac:table_026:0","source":"~C05オプション~C00","translation":"~C05选项~C00","context":"选项菜单标题"}
```

多字符串记录使用长度与布局 `parts` 完全一致的数组；`null` 表示该 part 不提供覆盖：

```json
{"key":"mhfdat:table_005:42","source":["名称","说明",null],"translation":["名称译文","说明译文",null]}
```

- `key` 必填，并在构建期根据 layout 校验表 ID、DAT/PAC 索引范围、quest ID 类型和
  part 数量。
- `translation` 可以是字符串、数组、显式空字符串或 `null`。只有非 `null` 的翻译会
  编译进启动器内嵌的二进制字典。
- `source`、`context` 和 `note` 是翻译辅助信息，不会嵌入启动器。
- 每个 locale 都可以只保留已经翻译的稀疏行；空文件会编译成没有覆盖的 locale。

构建期由 layout 计算合法的主资源 key；运行时按相同 layout 遍历实际存在的非空字符串
槽位。`missing = "key"` 的标记按需生成，并连续追加到地址稳定的独立 arena 中。

主资源 hook 在游戏完成资源内部指针重定位后、首次消费或复制前运行；layout 中的指针字段
此时都按绝对地址读取，非空槽直接写入独立翻译缓存的绝对地址。字典格式、key、layout 和
UTF-8 文本都在构建期校验；运行时不扫描或判断原字符串内容，只检查资源身份与外部内存边界。

## Native text syntax

翻译中直接书写游戏原有语法。printf 占位符（如 `%s`、`%d`）、波浪号控制（如
`~A`、`~C02`、`~K123`）和花括号控制（如 `{I1}`、`{K2}`、`{u12}`）都按原始 ASCII
字节透传，不做严格语义检查。

JSON 转义完全由 JSON 解析器处理；解析后的 ASCII 字符会直接编译为同值字节，例如
`\n` 为 `0x0A`、`\u001A` 为 `0x1A`。若需要字面量反斜杠则写 `\\n`；`\u0000`
会生成 NUL，游戏会把它视为字符串结束符。

`build.rs` 生成的二进制字节块保留翻译的 UTF-8 文本并通过 `include_bytes!` 直接进入 EXE。
启动时只读取所选 locale，并按 Unicode East Asian Width 的 CJK 宽度将非 ASCII 字符分为
半宽和全宽：半宽字符使用单字节虚拟字形 ID 和 8 像素槽，全宽字符使用双字节虚拟字形 ID
和 16 像素槽。所有 ID 在启动时一次性分配并在进程内保持固定，当前 locale 随后编码到独立
连续缓存。GDI hook 根据游戏传入的字节数查回 Unicode 字符并调用宽字符接口绘制。运行时
不读取外部字典、不解析 JSON，也不复制进原资源缓冲区。

## Generating a locale template

从已解密、已解压的资源 image 生成对应 locale 的翻译模板：

```text
python3 tools/generate_translation_dictionary.py \
  --locale zh-CN \
  --dat /path/to/mhfdat.bin \
  --inf /path/to/mhfinf.bin \
  --pac /path/to/mhfpac.bin
```

`--locale zh-CN` 默认生成 `translations/zh-CN.jsonl`；也可以使用 `zh`、`chs` 或
`my_translation` 等自定义 ID。原始日文写入 `source`；目标文件中已有的翻译字段和未由
主资源生成的记录会保留，因此可以再次运行生成器刷新原文。通过 `--output-dir` 可以指定
其他输出目录。游戏资源只在执行生成器时需要；正常 `build.rs` 只读取
`resources.json` 和 `translations/*.jsonl`。

## Missing translations

- `original`：没有覆盖时保留游戏资源中的原文。
- `key`：主 DAT/INF/PAC 的未翻译非空槽位显示动态标记，例如
  `[mhfdat:table_000:12]`；多 part 记录的后续 part 会追加 `:01`、`:02`。
- `empty`：把主资源中的未翻译非空槽位替换为空字符串。

Stage TLK 没有静态 layout，只处理字典中明确给出 `translation` 的 stage key；
没有列出的 TLK 记录保持原文。

翻译覆盖与游戏原生 `[localization] language` 相互独立。以日文资源制作的字典通常仍让
游戏语言保持 `japanese`，这样 `missing = "original"` 会回退到日文。
省略 `[translation]` 会关闭整套翻译 hook。
