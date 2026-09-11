# 场景资源

`stage` 中的类型描述磁盘结构，不创建原生场景，也不修改素材。`parse` 保留原文和
未消费尾部；无签名结构使用更严格的 `probe`，避免将任意数字数组当作已识别资源。

## 旧版场景环境与渲染表

`1089EF20` 将外目录 entry 0 交给 `108E0460`，后者读取内目录 entry 2 的
`LegacyLighting`。它与 HD `Lighting` 使用不同的版本编码和布局；只有一个短版本
字段不足以全局识别，因此该类型仅提供明确上下文中的 `parse`，不提供 `probe`。

| 偏移 | 磁盘字段 |
| --- | --- |
| 0 / 1 | u8 version / 未知 byte |
| 2 / 6 / 10 | 原始颜色 DWORD / 两个 float 的原始 bits |
| 14 | 108 字节，三组各三个 XYZ 向量，全部保留 u32 bits |
| 122（v2 起） | 48 字节，四个原始 DWORD 及八个原始颜色 DWORD |
| 170（v3） | u32 n、n 个 24 字节记录、u32 m、m 个 16 字节记录 |

`108E00C0` 精确复制 108 字节，`108DE7B0` 按向量组消费；`108E01C0`
读取后续 48 字节，并对最后八个 DWORD 做颜色字节重排。解析器保留磁盘 bits，
不执行该运行时转换。`108E0370` 的 24 字节记录为五个 DWORD 和两个 WORD，
16 字节记录为四个 DWORD；未证实含义的标量使用数值字段名。
v1/v2 的消费长度分别为 122/170 字节，v3 为 `178 + 24*n + 16*m`；额外尾部
保留为借用切片，计数与完整范围验证在记录分配之前完成。

已检查 `st001.pac` 和 `st017.pac` 的原始 v2 成员，均精确消费 170 字节。
其颜色 DWORD 分别为 `FF2F2320` / `FF1F1D18`，两个 float 分别为
`0 / 60000` 与 `0 / 55000`；这些观测不作为类型识别限制。

`1089EF20 → 113E8DA0` 读取场景目录固定 entry 2（目录头 +16/+20），并交给
`113FD190` 的 `LegacyRenderTables`。头长 16 字节，六个 u16 count 在
`+2,+4,+6,+8,+10,+12`，物理 stride 为 `24,32,4,32,68,16`；`+0` 和 `+14`
两个原始 WORD 分别保留为 `version` 和 `control`，不根据当前样本限制取值。
第六表的 16 字节消费由 `113FB490/113FD480` 确认。`st001.pac` 的数量为
`[3,3,8,1,3,0]`，精确消费 452 字节。该类型同样仅提供上下文 `parse`；
表记录和尾部借用源切片，不放宽 HD `RenderTables` 的 32 字节头或版本检查。

## HD 光照与渲染表

`1001F430 → 10021670` 单独加载 `stage-hd/st%03d-hd.pac`。实际 418 个包中，
417 个含四项，`st105-hd.pac` 含两项。它们没有 FMOD/FSKL，不能作为独立模型组：

| 项 | 原生使用 | 结构 |
| --- | --- | --- |
| 0 | LightManager、PostEffectManager | `Lighting` |
| 1 | `D3DXCreateCubeTextureFromFileInMemory` | 立方体纹理 TXB |
| 2（可缺省） | `11394DA0` | `RenderTables`，也可为空槽 |
| 3（可缺省） | 同一立方体纹理创建入口 | 第二组 TXB，允许 count=0 |

`Lighting` 完整支持已验证的 float 版本 3.2、3.3、3.4。12 字节头中，`+4..+8`
是五个独立的 **signed byte** 数量；`+9..+11` 是保留字节，实样常为 FE/CC，
不能把这四字节整体读成动画组数。依次为：

| 数据 | 每项长度 |
| --- | --- |
| PointLight | 104 |
| 两组三个 DirectionalLight | 各 24 |
| CubeMapLight | 40 |
| LightGroup | 28 |
| LightColision | 68 字节头 + 头末 signed count 指定的 u32 成员 |
| PointLightAnimGroup | 28 字节头 + channels |

动画组头 `+20` 是原始 DWORD channel 数；3.2 的 channel 头为 12 字节，
3.3/3.4 为 8 字节，头末 signed count 指定后续 16 字节关键记录数。
随后解析 Godray、HeightFog、DepthFog、DOF、Bloom、阴影/CSM、SSAO、
GaussianBlur 和 ToneMapping。DOF 在 3.4 从 44 扩展为 52 字节。
ToneMapping 是 1 字节 signed count、8 字节控制点、最后一个 DWORD；控制点
紧跟该字节，**不补齐对齐**。类别名来自原生 RTTI，未确认的标量保留原始 words。

418 个光照实样全部精确消费：3.2 为 1 个，3.3 为 239 个，3.4 为 178 个。
`st001-hd.pac` 的光照为 2393 字节，数量 `[5,13,30,0,0]`，ToneMapping 含 3 点。

`RenderTables` 的 32 字节头含 version、未知 `+2/+30` 字段及 13 个 u16 count。
物理顺序 `(count 偏移, stride)` 为：
`(4,28),(28,16),(6,40),(8,60),(10,36),(12,32),(14,44),(16,28),`
`(18,52),(20,20),(22,28),(24,12),(26,16)`。181 个 v2 实样全部精确消费。
`st037-hd.pac` 和 `st258-hd.pac` 为 v1；当前客户端明确拒绝 version <2，
本解析器也明确报告未支持，未假造其 payload 布局。

## 场景摆放目录和共享成员

`StageArchive` 的头是三组固定 offset/size 和 `u32 additional_count@24`。
**附加目录从 +28 开始**，每条为 `(resource_id,offset,size)` 三个 DWORD。
旧参考实现将目录错误地从 +32 开始，因此虽然 offset/size 恰好可读，资源 ID 会错位。
`113E8DA0` 从 +28 读取 ID，并用它匹配 60 字节摆放记录的 `u16 resource_id@54`。

slot 0 的 `PlacementTable` 为 16 字节头及 count 个 60 字节记录：
`version=2,count,unknown_08,unknown_0c`。225 个实样的 `unknown_08` 均为 FFFFFFFF；
`unknown_0c` 保留，不固定成哨兵。`StageArchive::probe` 要求这个表完整匹配，
而非单纯以“目录没有越界”识别整个资源。

附加项通常是 `ObjectPackage`。`113DA8C0` 读取其普通 offset/size 目录的第 0 项：
`u16 unknown_00,u16 count,u8 kinds[count]`，每个 byte 对应包内第 1..N 项。
`probe` 要求原始 index 头值 1、完整 kinds、数量与外目录相符以及已知 kind。
`parse` 仍保留未被 index 使用的外目录项和 index 尾部。

| kind | 原生角色 |
| --- | --- |
| 1 / 2 / 3 | FMOD / FSKL / TXB |
| 4 | `ObjectTables` |
| 5 / 6 | HITS 碰撞 |
| 7 | KEFFECT |
| 8..11 | MOT |
| 13 | 特效目录 |
| 14 | `ObjectWordTable`，含义未知的 DWORD 表 |

kind 14 的观测结构是 `u32 unknown_00@0`、`u32 count@4` 和 `count` 个 DWORD。
290 个实样全部精确满足 `8 + 4*count`，count 也都与同包 kind 3 TXB 的纹理数量
一致；这个对应关系不证明 value 的含义。`113DA8C0` 的 case 14 跳到临时缓冲区
释放路径，没有解释这些值，故不将其命名为哈希或骨骼索引。`ObjectWordTable`
仅由 kind 14 上下文调用 `parse`，保留任意 unknown_00、原始 values 切片和尾部，
`values()` 只按小端 u32 迭代，不增加全局 `probe` 或纹理数量限制。

成员开头的 `FF FE FD FC FB FA F9 F8 F7 F6 F5 F4 F3 F2 F1 F0` 加 `u32 resource_id`
是 `ResourceReference`，**不是空条目**。`113DA650` 在同一 StageArchive 的目标 ID
对象包中查找相同 kind。`resolve_member` 返回原始 source 的借用切片、绝对文件偏移和
引用链，迭代检查循环、缺失 ID 和缺失 kind，不拼造文件、不替换成邻近项。
SD 综合实样验证覆盖 225 个目录、1730 条摆放、1418 个对象包；1285 个跨资源成员
全部解析到原始 source 中的目标切片，没有缺失目标或循环。

`ObjectTables` 对应 `113DA520`：16 字节头、version>=8，五个 u16 count 在
`+2,+4,+6,+8,+10`，物理表 stride 为 `68,24,16,32,16`。
最后一表的 16 字节步长由 `113DDB90/113DE180/113DE3F0` 直接确认。
1286 个直载 v8 表全部精确消费；另有 132 个 kind 4 成员是跨资源引用。

## HITS 与 KEFFECT

HITS 有 40 字节头。`108C78A0/108C7960` 将两个目录偏移和 cell 指针相对 **file+8**
重定位；每个 cell 指向以 FFFFFFFF 终止的记录偏移链。链项相对 records 区，
每条碰撞记录 56 字节：未知 DWORD、三个 xyz 顶点、四个平面系数。
`108C7B90` 复制 56 字节；`108CBE30` 直接使用这些顶点和平面；空间网格查询按 x/z。
解析器验证 cell 数量、链哨兵、引用对齐和所有范围，保留合法的未引用记录。
1098 个实样全部通过：457717 条 records、2784878 个 cell 引用。

KEFFECT 的前七字节由 `105EECB0` 检查；第八字节仍保留。16 字节头中的
`u32 count@12` 指定 112 字节关键记录。`105EECF0` 按 `kind@0,target_id@4,frame@8`
查找相邻记录，`105EED60` 取最大 frame；`105EEED0` 使用 `render_mode@64`。
其余 words 保留，不能把它与同为 112 字节的其他发射器格式混为一谈。
56 个 KEFFECT 实样、2719 条关键记录全部完整消费。

## 场景区域相机

`AreaCamera` 对应普通场景外层目录的 slot 29。`1089EF20` 解开 JKR 后交给
`1081FDF0` 重定位；`1081FE90/1082F860` 按角色位置选取区域，`1082F910`
查询其中的 64 字节子记录。这与逐帧事件相机 `EventCamera` 使用不同布局。
解析仅由场景上下文选择，当前支持已确认的 `0x0102` 版本，不提供全局 probe。

头部 48 字节，包含版本、区域数、空间网格参数，以及 `+28/+32` 的网格目录和
引用池偏移。区域从 +48 顺序排列；每条为 768 字节固定部分、`u8@5` 指定数量的
64 字节子记录，以及 `u32@28` 非零时的 32 字节扩展。区域内部指针相对该区域。
每个网格目录项为两个 DWORD：引用数量和相对整个资源起点的列表偏移；列表
存放区域记录的资源内偏移。空列表保留原始指针位值，其公开空切片仍位于源数据中。
未确认的参数及头部 `+36/+40/+44` 原样保留，不把 `+36` 猜成文件总长度。

`st001.pac` 的实样为 5292 字节，包含 6 个区域、1 个空间网格和 5 个区域引用，
区域起点为 `48/816/1808/2672/3536/4400`；网格和引用池分别从 5264、5272 开始。

## 验证和边界

`tests/stage_native_samples.rs` 用 `MHF_RESOURCE_GAME_ROOT` 只读验证原始 SD/HD 场景。
常规测试包含 count 溢出、每个截断边界、未知字段、原始 float bits、目录 ID、
跨包乱序成员与引用循环。游戏素材不加入仓库。

这些格式支持不代表目录内每个辅助资源都已还原。无证据的剩余短表、旧版本及未知
成员仍保留原字节和明确的未支持状态；不以文件名、魔数扫描或重命名掩盖缺失。
