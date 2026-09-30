# 文本表布局

[`resources/layout.json`](../resources/layout.json)（schema v4）描述 DAT、INF 等八个
客户端资源的字符串表结构。Workbench 构建脚本通过
[`build/resource_layout.rs`](../build/resource_layout.rs) 校验这些定义，展开目录并生成
`OUT_DIR/dat_text.rs` 与 `OUT_DIR/inf_layout.rs`，供 DAT 与 INF 检查器使用。
显示层按 CP932 解码日文原文；解析器保留原始字符串和混合记录中的非文本字节。
DAT 的检查范围见 [DAT 格式](dat-format.md)。

文件顶层的 `version` 指定 schema 版本，`record_layouts` 保存共用记录结构，
`resources` 按资源声明表目录。资源的 `type` 区分普通记录表 `records` 和 INF
任务目录 `quest`；`identity` 保存 magic 与格式版本，没有这类头部的资源使用 `null`。
固定代码页可通过 `code_page` 声明。

`runtime` 保存所支持客户端 DLL 的构建绑定信息：`post_relocation` 含指令 RVA 和
字节签名，`buffer_rva`、`size_rva` 是资源地址与大小字段的 RVA。RVA 使用 `0x`
前缀的十六进制字符串；签名以空格分隔字节，`??` 表示通配字节。离线检查保留并校验
这些元数据，只生成资源表布局，不安装 Hook。

## 共用记录结构

相同形状的记录在 `record_layouts` 中定义一次，由各表的 `layout` 引用：

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

文本字段使用客户端的 32 位指针布局；其他字段仍属于原始记录，不因包含文本而被忽略。

## 普通记录表

记录资源的表字段示例（省略 `runtime`）：

```json
{
  "id": "mhfdat",
  "identity": { "magic": 442919021, "format_version": 89 },
  "type": "records",
  "tables": [
    {
      "id": "hunter_guide_sections",
      "root_field": 416,
      "records": 4,
      "layout": "text_1_at_0_stride_12"
    }
  ],
  "table_directories": []
}
```

- `id` 是同一资源内唯一的稳定表标识；未知业务含义使用中性 ID。
- `root_field` 是资源 image 中保存记录表指针的字段位置。数组表示间接指针路径，
  例如 `[176, 8]` 先解引用 image+176，再读取目标+8 的记录表指针。
- `first_record` 默认为 0，表示同一物理表中逻辑分段的起始记录。
- `records` 根据文件结构读取。整数表示固定数量；`{"u16_at":[0,6]}` 和
  `{"u32_at":16}` 分别从指定字段读取数量。
- 哨兵计数使用
  `{"until":{"root_field":16,"stride":72,"offset":0,"width":2,"value":65535}}`，
  表示沿元数据表逐记录读取 2 字节字段，遇到 `65535` 结束；零指针终止表使用
  `width: 4`、`value: 0`。
- `directory` 检查间接表的外层记录是否存在，先判断 `index < count` 再解引用子表，
  例如 `{"index":1,"count":{"u32_at":180}}`。GAO 的地点组行数仍从目录中读取。

数量、物理起始记录和目录存在性来自布局与文件，不能从显示节点数或字符串内容推断。

## 连续根目录

多个相邻根字段指向相同形状的记录表时，使用 `table_directories`：

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

每个 entry 的根字段为 `first_root_field + entry_index * root_stride`。
构建期将目录展开成普通表；这种写法只压缩定义，不改变记录位置或表内索引。

## INF 任务目录

INF 使用独立的 `quest` 结构，其布局字段示例（省略 `runtime`）：

```json
{
  "id": "mhfinf",
  "identity": { "magic": 442920553, "format_version": 6 },
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

`root_field` 指定分类目录指针字段，`count_root_field` 指定分类数量的指针字段；
分类数量从后者指向的位置按 `u16` 读取。
每个分类按 `category_stride` 读取，`category_count_field` 与
`category_records_field` 分别指定分类内的槽数量与任务指针槽表的指针字段。
槽表的非零项指向独立任务记录，不据此推定任务记录具有固定步长。
任务记录中的 `record_text_field` 指定文本指针表的指针字段，该表含 `parts` 个文本指针；
`record_id_field` 指向记录自身的 `u16 questID`。任务身份使用这个原始 ID，
不从目录位置或显示名称推导。

## 检查约束

构建期检查 schema 版本、记录布局引用、表 ID 唯一性、字段范围与目录定义。
DAT 检查器读取文件时继续检查指针、记录数量和字符串范围；空指针、未知字节及
非文本字段保留在源数据中。打开表后按需建立记录节点，字段展开时再读取文本。
