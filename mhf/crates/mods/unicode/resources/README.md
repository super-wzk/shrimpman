# MHF text resources

`mhf-unicode/provider` 负责资源和 DLL 原生文本转码、Unicode 字符边界、输入编辑及绘制。
它不要求启用 Translation provider；所有原文按明确的源代码页转为 UTF-8。
字体资源、注册和度量修正属于 Font Mod。

Unicode 构建脚本读取 `layout.json` 与 `src/provider/resources/native/layout.rs`，
由 `build/resource_layout.rs` 统一解析并生成 `OUT_DIR/resources.rs`。Translation 的
构建脚本复用同一个 catalog 解析源及此份资源数据，独立生成词典；不会建立
Translation → Unicode Cargo provider 依赖。Unicode 运行时只携带稳定字符串 Key，
词典 ordinal 保留在 Translation 实现内部。

## Layout

`layout.json`（schema v4）描述八个资源的字符串结构及其在目标客户端中的运行时绑定。相同的
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

每个资源用 `type` 明确选择一种结构。除 INF 的任务目录外，其他资源均使用 `records`：

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
- `root_field` 是资源 image 中保存记录表指针的字段位置。数组表示指针路径，例如
  `[176, 8]` 先解引用 image+176，再读取目标+8 的记录表指针。
- `first_record` 默认为 0，表示同一物理表中这一逻辑分段的起始记录；翻译 key 仍从 0 开始。
- `records` 从资源结构读取，不能根据翻译字典的内容推断。整数表示加载器固定数量；
  `{"u16_at":[0,6]}` 和 `{"u32_at":16}` 分别从指定字段读取数量。
- 哨兵计数使用 `{"until":{"root_field":16,"stride":72,"offset":0,"width":2,"value":65535}}`，
  表示沿元数据表逐记录读取 2 字节字段，遇到 `65535` 结束；零指针终止表使用 width=4、value=0。
- 间接目录中的表还使用 `directory` 检查外层记录是否存在，先判断 index<count 再解引用子表：
  `{"index":1,"count":{"u32_at":180}}`。GAO 的 31 个地点组保留稳定 ID，每组行数仍从目录中读取。

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

## Unicode 转码

主资源 hook 在游戏完成资源内部指针重定位后、首次消费或复制前运行；layout 中的指针字段
此时都按绝对地址读取。原文沿已声明的字符串指针读取，在资源边界内找到 NUL 后，按文件来源的代码页转成 UTF-8。同一代码页中的
相同原文字节共用稳定缓存，
ASCII 字符串保留原指针。已经指向资源外 UTF-8 缓存的槽不再转码，不猜测混合编码。
资源 layout 在构建期校验；转码不依赖翻译词典。

主 DAT/INF/PAC 由游戏语言字段选择整份资源：`japanese` 和 `english` 使用 CP932，
`korean` 使用 CP949，`traditional_chinese` 使用 CP950；这与原客户端字体字符集分支一致。
PAC 转换后再次调用原生文本缓存构建函数，更新提前缓存的六个标签，并保留特殊状态下
第五项来自另一张表的选择逻辑。

JMP/GAO/SQD/RCC/MSX 没有 DAT/INF/PAC 的 magic/version 头，`identity` 为 null；
这五个固定日文资源明确使用 `code_page: 932`，不跟随启动语言猜测编码。它们分别在各自
加载器完成解密、解压和全部指针重定位之后转换。原始零文本偏移有时被加载器无条件加上
image 基址，这些槽保持原值，不把文件头解析成字符串。

| 资源 | 文本范围 | 非空文本字段（当前文件） |
| --- | --- | ---: |
| JMP | 移动目的地名称、说明、公共标签 | 53 |
| GAO | 伙伴猫装备名称／说明、八组台词、地点目录、技能表 | 2881 |
| SQD | 技能、效果、次数、星级、名称等七组表 | 253 |
| RCC | 活动标题、说明与公共标签 | 36 |
| MSX | 颜色、物品名称及效果 | 40 |

翻译可选地覆盖这些字符串槽；词典格式、语言选择和缺失译文策略见
[翻译说明](../../translation/locales/README.md)。
