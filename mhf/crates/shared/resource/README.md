# MHF 资源格式

`mhf-resource` 用 Rust 类型表示客户端资源文件，在原始字节上通过
明确的字节序读取字段，不依赖 Windows 或游戏进程。`binary::Reader` 提供统一的类型化
读取入口，`read::<u16>()`、`read::<[f32; 3]>()` 返回包含值、原始字节引用、绝对字节范围
和端序的 `binary::Field<T>`；整数、浮点与数组的读取和写回共用 `BinaryValue`。
原始 buffer 仍是完整数据来源，类型化读取不会丢弃未知字节或浮点位模式。
文件偏移不转换为宿主指针。未知字段、原始位标记、空槽、别名和未识别块保留在源数据中。

| 模块 | 数据 |
| --- | --- |
| `binary` | 带来源位置的类型化字段、游标／偏移读取与统一数值编解码 |
| `crypto` | ECD、EXF 头与编解码、文件名校验及 ECD 内容 CRC |
| `jkr` | JKR 原始、Huffman、LZ、HFI 编码与边界检查 |
| `container` | offset/size、MOMO、MHA 命名目录及资源 ID 索引、场景专用目录与嵌套封装 |
| `dat` | DAT v89 根结构、装备／物品／生产记录、特效绑定与定义表，以及按布局解析的文本记录 |
| `inf` | INF v6 任务分类、任务指针槽、原生 ID 查找及任务文本 |
| `fmod` | 对象、顶点、法线、UV、颜色、权重、骨骼映射、三角带、材质、贴图引用、18字渲染参数块 |
| `fskl` | 原始节点序号、层级索引、根节点目录、变换、动画分组标签与未知尾部 |
| `motion` | 显式组数的 MOT 目录、经完整验证的文件目录记录、动作、轨道、通道及六种关键帧编码 |
| `material` | 模型包内独立的分组材质参数，保留96/100字节记录 |
| `effect_archive` | 包内特效索引、发射器与曲线、定义字段与原生曲线查找、动作事件及稀疏索引 |
| `effect` | DAT 160、161、165、166 的独立定长装备效果记录 |
| `stage` | 旧版环境与 HD 渲染动画表、光照与后处理、区域相机、HITS 碰撞、KEFFECT 关键记录、场景摆放、对象包种类与跨资源引用 |
| `txb`、`png`、`dds` | 纹理目录、原始 PNG 块与 CRC、DDS 头及 mip/面/数组/体积范围 |

`resources/layout.json` 和 `resources/native/` 保存共用的文本布局与原生地址定义。
`build/resource_layout.rs` 供 Unicode、Translation 和 Workbench 构建脚本复用，
分别生成运行时资源绑定、校验词典 Key 和生成 DAT 检查布局。布局说明见
[资源布局与转码](../../mods/unicode/resources/README.md)。

模型和动作的局部编辑 API 返回字节副本，只修改对应字段。
纹理提取保留 PNG/DDS 文件原文。解析器不重排数据、归一化权重或为未知字段补造语义。
MOT 的原生消费组数由客户端调用方传入，不能从扩展名统一设为常数。
`ObservedMotionDirectory` 只描述经完整边界和动画验证的文件记录区域，包含尾部空记录，
不把记录数冒充为游戏实际消费组数。

详细布局、原生读取函数与未确认项见
[`docs/model-formats.md`](docs/model-formats.md)、
[`docs/motion-effects.md`](docs/motion-effects.md) 与
[`docs/dat-format.md`](docs/dat-format.md)、
[`docs/inf-format.md`](docs/inf-format.md)、
[`docs/container-formats.md`](docs/container-formats.md)、
[`docs/material-parameters.md`](docs/material-parameters.md) 与
[`docs/effect-archives.md`](docs/effect-archives.md) 与
[`docs/stage-formats.md`](docs/stage-formats.md)。测试文件中的真实资源路径通过环境变量提供，
游戏素材不加入仓库。未识别资源由工作台保留为原始字节，不标记为已还原格式。

```sh
cargo test --manifest-path mhf/Cargo.toml -p mhf-resource --target aarch64-apple-darwin
```

在其他系统上将目标替换为对应的宿主三元组。

从子资源读取时使用 `Reader::with_base(slice, offset)`；字段范围包含这段数据在完整
buffer 中的起点。`Field::write` 的目标是完整 buffer。默认小端，通过
`with_endian(Endian::Big)` 明确选择大端；失败读取不推进游标。工作台根据 `BinaryValue`
的类型元数据构造 `Binding`，因此 UI 与写回不需要重复推断字段宽度或端序。
