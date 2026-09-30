# MHF 资源格式

`mhf-resource` 用 Rust 类型表示客户端资源文件，在原始字节上通过
明确的字节序读取字段，不依赖 Windows 或游戏进程。`binary::Reader` 提供统一的类型化
读取入口，`read::<u16>()`、`read::<[f32; 3]>()` 返回包含值、原始字节引用、绝对字节范围
和端序的 `binary::Field<T>`；整数、浮点与数组的读取和写回共用 `BinaryValue`。
原始 buffer 仍是完整数据来源，类型化读取不会丢弃未知字节或浮点位模式。
文件偏移不转换为宿主指针。未知字段、原始位标记、空槽、别名和未识别块保留在源数据中。

## 模块职责

| 模块 | 数据 |
| --- | --- |
| `binary` | 带来源位置的类型化字段、游标／偏移读取与统一数值编解码 |
| `path` | 游戏相对文件与原始内部索引／稳定 schema 字段组成的资源身份 |
| `crypto` | ECD、EXF 头与编解码、文件名校验及 ECD 内容 CRC |
| `jkr` | JKR 原始、Huffman、LZ、HFI 编码与边界检查 |
| `container` | offset/size、MOMO、MHA 命名目录及资源 ID 索引、场景专用目录与嵌套封装 |
| `dat` | DAT v89 根结构、装备／物品／生产记录、特效绑定与定义表，以及按布局解析的文本记录 |
| `action_definition` | DAT[389] 武器招式、原始步骤／事件、动画引用与 SDT 攻击查找条件 |
| `dat::motion_events` | DAT[390]／[391] 两级动作事件目录、16 字节操作事件与 22 字节分派事件 |
| `emd` | 物种记录、按头部计数的根表和参数目录 |
| `event_camera` | 逐帧事件相机的视野角、位置、滚转角与目标数组 |
| `inf` | INF v6 任务分类、任务指针槽、原生 ID 查找及任务文本 |
| `sdt` | SDT 攻击／辅助参数、八槽判定组、球体／胶囊与条件指令、武器修正和类别专用状态／运动参数 |
| `fmod` | 对象、顶点、法线、UV、颜色、权重、骨骼映射、三角带、材质、贴图引用、18字渲染参数块 |
| `fskl` | 原始节点序号、层级索引、根节点目录、变换、动画分组标签与未知尾部 |
| `motion` | 显式组数的 MOT 目录、经完整验证的文件目录记录、动作、轨道、通道及六种关键帧编码 |
| `material` | 模型包内独立的分组材质参数，保留96/100字节记录 |
| `effect_archive` | 包内特效索引、发射器与曲线、定义字段与原生曲线查找、动作事件及稀疏索引 |
| `effect` | DAT 160、161、165、166 的独立定长装备效果记录 |
| `stage` | 旧版环境与 HD 渲染动画表、光照与后处理、区域相机、HITS 碰撞、KEFFECT 关键记录、场景摆放、对象包种类与跨资源引用 |
| `txb`、`png`、`dds` | 纹理目录、原始 PNG 块与 CRC、DDS 头及 mip/面/数组/体积范围 |

`resources/layout.json` 保存共用的文本布局与客户端 DLL 绑定信息。
Workbench 构建脚本通过 `build/resource_layout.rs` 校验布局并生成 DAT 与 INF 检查表。
字段与目录定义见 [文本表布局](docs/text-layout.md)。

模型和动作的局部编辑 API 返回字节副本，只修改对应字段。
纹理提取保留 PNG/DDS 文件原文。解析器不重排数据、归一化权重或为未知字段补造语义。
MOT 的原生消费组数由客户端调用方传入，不能从扩展名统一设为常数。
`ObservedMotionDirectory` 只描述经完整边界和动画验证的文件记录区域，包含尾部空记录，
不把记录数冒充为游戏实际消费组数。

## 资源路径

`ResourcePath` 的规范表示是 `文件#内部路径`：`motion/w04.mot#4/5`、
`mhfsdt.bin#0/attacks/23`、`mhfemd.bin#17/42`。内部段只有原始零基索引
`PathSegment::Index(u32)` 和稳定 ASCII schema 标识 `PathSegment::Field(String)`；
格式适配器赋予段具体含义，不能用 UI 节点顺序、显示名称或未确认的 native 地址构造身份。
未知字段标识原样保留，不在解析时跳过；ECD／EXF／JKR 等透明编码不会自动增加层级。

`ResourcePath::new` 和 `from_parts` 接收未转义的 UTF-8 游戏相对文件路径。
反斜杠规范为 `/`，保留大小写、Unicode 与空格；拒绝绝对路径、驱动器路径、
空段、`.`／`..` 段和控制字符，不访问或规范化真实文件系统。规范文本只将来源中的
`#`／`%` 转义为 `%23`／`%25`。`FromStr` 严格解码来源的百分号转义，索引按
十进制解析并去除前导零；字段遵循 `[A-Za-z_][A-Za-z0-9_]*`，不接受显示标签。
纯文件身份没有 `#`；空片段和空内部段无效。
原始名称字节、raw ID、数据层字节范围与未解析 native 引用仍由各格式数据保留，
资源路径不将这些值互相转换或猜测为索引。

DAT／SDT 的 `FieldLayout.key` 是显式稳定字段标识，`name` 继续保存显示标签。
已确认的 SDT 攻击核心字段使用 `startup_count`、`active_count`、`power`，
已有 `unknown_*` 标识保留；其余字段使用声明中的 `field_XX` 原始记录偏移键。
同一实际记录 schema 内的键唯一，不从中文标签翻译或从界面节点序号推导。

```rust
use mhf_resource::{PathSegment, ResourcePath};

let path = ResourcePath::from_parts("mhfsdt.bin", [
    PathSegment::Index(0),
    PathSegment::Field("attacks".into()),
    PathSegment::Index(23),
]).unwrap();
assert_eq!(path.to_string(), "mhfsdt.bin#0/attacks/23");
```

## 招式定义与原生引用

`action_definition::Definition::parse(bytes, base, weapon, action)` 读取 DAT[389]。
离线文件传 `base = 0`，重定位后的 DAT 传其原生基址；返回偏移始终相对于输入字节，
不保留宿主指针。`weapon_actions` 单独读取武器目录，支持 14 个武器、每个最多 256 个招式；
步骤、派生条件和事件各最多 4096 条，零计数不解引用其指针，非空表校验完整范围。
`ActionStep` 保留六个原始 `u16`，`ActionTransition` 保留 40 字节的优先级、
输入／选择编号、调用参数及四个八字节条件窗口；`ActionEvent` 保留 12 字节的步骤、时机、
有符号 phase、帧、次数、操作码与参数，均支持原样写回。来源路径和范围使用原始序号。
派生表来自招式头 `+8/+12` 的数量／指针；窗口的原生消费证据见
[招式定义](docs/action-definitions.md)。
这些招式事件与特效资源的动作事件使用各自的原生布局，不互相转换。

`NativeMotionRef` 保留原始动画 ID、武器与风格；已确认的 bank 返回规范 MOT 路径，
未知 bank 或缺少必要上下文返回 `None`；旋棍武器 bank 1 还要求风格为已确认的 0–3。
`AttackReference` 保存 category、可选 subtype 与记录号，`AttackDirectory::resolve` 查询实际 SDT 目录，
普通类别查询按原生稳定排序选择 subtype 最小的首项，显式 subtype 查询精确匹配，
再以该条目的原文件 `entry.index` 生成路径，不使用运行时排序索引。
不存在的类别或攻击表返回 `None`，
损坏的表或越界记录返回错误；不从类别编号猜测目录槽。

`AttackDirectory::from_sdt(source, file)` 将原文件目录的索引、类别键、子类别键和
攻击记录数量／范围校验结果保存为可克隆的独立值，源 SDT 字节释放后仍可解析引用。
它不保存原生指针，不采用运行时重排后的目录地址或序号；同一类别的 subtype 逆序
以及同键重复项仍按上述原生选择规则查找，返回原文件路径，例如
`mhfsdt.bin#2/attacks/8`。攻击表损坏保留在对应条目的校验结果中，
只有引用选择该条目时才返回错误，不阻止其他有效条目生成路径。
应用可在资源读取线程建立并共享该目录，编辑 SDT 草稿后重新建立目录；
公共 UI 接收解析得到的 `ResourcePath`，动画和攻击目标共用规范路径主值与复制行为。
详细布局见 [DAT 格式](docs/dat-format.md)。
动作事件目录及其原生消费证据见 [DAT 动作事件](docs/dat-motion-events.md)。

## 格式文档

| 主题 | 文档 |
| --- | --- |
| 外层编码与目录 | [容器格式](docs/container-formats.md)、[资源寻址与打包布局](docs/resource-alignment.md) |
| 模型与材质 | [模型格式](docs/model-formats.md)、[材质参数](docs/material-parameters.md) |
| 动作与效果 | [动作数据](docs/motion-effects.md)、[特效库](docs/effect-archives.md)、[装备特效动画](docs/equipment-effect-animation.md) |
| 游戏数据表 | [DAT](docs/dat-format.md)、[INF](docs/inf-format.md)、[EMD](docs/emd.md)、[SDT 战斗参数](docs/attack-parameters.md) |
| 场景数据 | [场景格式](docs/stage-formats.md) |

各格式文档列出布局、原生读取依据与未确认项。未识别资源由工作台保留为原始字节。

## 验证

从仓库根目录运行：

```sh
cargo test --manifest-path mhf/Cargo.toml -p mhf-resource --target aarch64-apple-darwin
```

在其他系统上将目标替换为对应的宿主三元组。真实资源验证需按各测试声明配置输入路径；
带 `#[ignore]` 的测试还需显式传入 `-- --ignored`。未提供样本时，普通测试通过不代表
真实资源已验证。游戏素材不加入仓库。

## 字段读取与写回

从子资源读取时使用 `Reader::with_base(slice, offset)`；字段范围包含这段数据在完整
buffer 中的起点。`Field::write` 的目标是完整 buffer。默认小端，通过
`with_endian(Endian::Big)` 明确选择大端；失败读取不推进游标。工作台根据 `BinaryValue`
的类型元数据构造 `Binding`，因此 UI 与写回不需要重复推断字段宽度或端序。
