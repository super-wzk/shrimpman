# 资源外层与偏移目录

本文记录 `container.rs`、`crypto.rs`、`jkr.rs` 的证据范围。固定头直接借用已检查边界的
源切片，压缩符号流按游标读取，整数使用明确的小端转换。不把磁盘结构强转为宿主指针，
不修改原文件。未知字段、头后间隙和文件尾保留在 `source`。

## Rust 透明包装

`Decoded<H, T>` 持有 `encoding: H` 和内部 `T`，实现 `Deref<Target = T>` 与
`AsRef<T>`。`T` 不限于字节，可以是拥有数据的结构或另一个 `Decoded`。
ECD、EXF、JKR 的 `decode(self, budget)` 实际返回
`Decoded<Self, Box<[u8]>>`；其中 `Self` 保留原始借用切片与对应编码头，解码输出
由 Box 单独拥有。原始 encoded 字节不被复制，也不会被解码输出替换。

```rust
let decoded = Jkr::parse(encoded)?.decode(usize::MAX)?;
let model = Fmod::parse(&decoded)?;
// model 借用 decoded 拥有的字节；decoded.encoding 保留 JKR 头和原文。
```

借用解析器不与其字节 owner 放进同一个自引用结构；保留 wrapper 后再借用解析即可。
`map_inner` 可将 Box 转交给 Arc 或其他内部类型，`map_encoding` 只变换元数据。
`open_layers` 的每层直接使用 `Decoded<LayerHeader, Box<[u8]>>`，原始来源唯一来自
外层 `source` 或前一层的内部字节，`layer_source` 返回精确输入。`OpenedResource`
也解引用到最终字节，所以可直接用于 `Fmod::parse(&opened)`。

工作台接收同一个泛型解码结果，将内部 Box 转为 Arc 后移交资源树的数据层；原始
编码头、来源范围及编码层节点继续保留，检查和导出不依赖重新编码。

## 证据来源

- [ReFrontier](https://github.com/Houmgaor/ReFrontier/tree/f293111cfb7573ee39001809623fb14290a96ae7)：
  `ReFrontier/Jpk/JPKDecodeRW.cs`、`JPKDecodeHFIRW.cs`、`JPKDecodeLz.cs`、
  `JPKDecodeHFI.cs`、`LibReFrontier/Crypto.cs`、
  `ReFrontier/Services/UnpackingService.cs`。
- 当前仓库 `mhf/crates/mods/quest/src/provider/binary.rs` 的 JKR3 解码与
  上述 LZ 指令一致；共享实现另外处理 Huffman 层、输出预算和截断错误。
- 只读资源测试使用 `MHF_RESOURCE_GAME_ROOT` 指定游戏目录。仓库不包含游戏资源本体。
- 独立比较使用 [MHFrontier-Blender-Addon 的解码器](https://github.com/Houmgaor/MHFrontier-Blender-Addon/blob/main/mhfrontier/stage/jkr_decompress.py)。
  其文件顶部说明的编码序号与实际枚举不一致，本实现依照实际枚举和文件。

## JKR

| 偏移 | Rust 值 | 已知含义 |
|---|---|---|
| 0x00 | `[u8; 4]` | `JKR\x1a` |
| 0x04 | `u16` | 版本；已验证的版本值为 `0x0108` |
| 0x06 | `u16` | 编码编号，未知编号仍可读头但不能解码 |
| 0x08 | `u32` | 编码数据相对整个 JKR 文件的偏移 |
| 0x0c | `u32` | 输出字节数 |

编号 `0` 为 raw，`1` 为 None/raw，`2` 为 Huffman-only，`3` 为 LZ，
`4` 为 Huffman-over-LZ。编号 4 的 LZ 控制字节也经过 Huffman 编码，两个
按位状态必须独立。Huffman 开头的 `u16` 是根节点编号；内部节点 N 的两个
子引用位于 `table_start + (N - 256) * 4`。因此根 510 对应 510 个 `u16`
子引用，而不是“510 个内部节点”。字节符号为 0..255。

读取前验证树引用与循环、数据范围和输出预算。LZ 指令不能读取未产生的输出，
允许重叠回溯，禁止越过声明输出长度；截断不会补零并报告成功。

游戏是否允许省略 JKR，由具体加载入口决定。装备的 FMOD/FSKL 双成员路径
`108DF570/108DF4B0` 分别检查成员 0 和 1 的 `JKR\x1a` 签名：存在时按头部
输出长度分配并解压，否则按目录 size 直接复制原始块。因此这条路径允许直接
存放未压缩 FMOD/FSKL；重打包仍需更新目录 offset/size。不能按“入口层/内部层”
推断可选性，例如部分 `.gab` 路径 `105E5D10` 直接按压缩头取长度并调用解码器。

## ECD 与 EXF

ECD 16 字节头：签名 `ecd\x1a`、`u16` key index、原样保留的 `unknown_06[2]`、
`u32` payload size、`u32` 解密后 CRC32。支持参考实现的六组参数，解密后
校验 IEEE CRC32。只处理声明 payload，外部尾字节仍在 `source`。

EXF 16 字节头：签名 `exf\x1a`、`u16` key index、`unknown_06[2]`、
`unknown_08[4]`、`u32` seed。支持五组参数，解密从偏移 16 到文件末尾。
参考实现不将 `unknown_08` 作为长度字段，本实现也不这样推断。EXF 可包装 Ogg
音频；`seed` 用于解密，不作为通用输出 CRC 校验字段。

`open_layers` 保留 original source 与每一层的 decoded bytes。原样导出应使用
`source` 或 `layer_source`。解密/解压后的字节并不等于原压缩文件，也不声称可以
从解码输出无损重建原编码。解码字节预算累计计算，额外有外层数量上限。

## 偏移目录

- BIN/TXB/PAC/GAB 常用目录：`u32 count`，随后 count 组
  `{offset: u32, size: u32}`。后缀不是格式签名；必须验证整个目录和数据范围。
- MOMO：`MOMO` 签名后接同样的 count 和目录。偏移仍相对于整个文件。
- Stage 特有目录：前三组 offset/size 固定在 0..24；`+0x18` 为附加项数；
  附加项从 `+0x1c` 开始，每组 12 字节，为 resource_id/offset/size。
  原生 `113E8DA0` 按 resource_id 匹配摆放记录，不能将第一条 ID 当成额外头字段。
  `parse` 是显式解释；结构识别用 `probe`，还要求 slot 0 是完整的场景摆放表。
  不能仅凭 `.pac` 扩展名或零值猜测。
- MHA：`mha\x01`，entry table offset、count、names offset、names size，
  再加两个未知 `u16`；项为 name offset、payload offset、size、padded size、
  file id，共 20 字节。name offset 相对 name block，其他偏移相对文件。
  名称保留原始字节，由显示层负责解码。目录可能在 payload 之后。

零长度槽原样保留（包括其非零旧 offset）；非空数据不能越界。目录允许别名、
间隙和尾部数据，不按“所有 size 之和必须等于文件长度”错误排除合法资源。
这些读取器不跟随操作系统路径，不执行归档项，不写回。

## 验证与识别边界

`tests/container_decode.rs` 覆盖 JKR 编码编号、LZ 指令、重叠回溯、Huffman 位流、
截断、循环引用、预算限制和未知编码。ECD 使用独立实现生成的已知答案覆盖各组密钥，
并检查损坏数据触发 CRC 错误；EXF 测试保留原始头和源字节。

目录测试检查空槽、别名、间隙、尾部、MHA 共享名称，以及 Stage 目录的独立布局
和摆放表识别条件。场景资源 ID 与共享成员引用的验证见 [stage-formats.md](stage-formats.md)。

可选资源测试覆盖 JKR0/3/4、ECD、EXF、模型成员、TXB、MHA 和 MOMO。
测试按资源相对路径读取原文件，并以指定参考资源的输出 CRC 检查解码回归；
这些校验值不用于解析器的类型识别。JKR2 的覆盖使用构造的 Huffman 已知答案，
不等同于游戏资源兼容性验证。

工作台从 `dat/` 文件树和已验证的容器目录建立资源树，不将 `mh2pc.dat` 解释为
虚拟文件目录，也不将任意资源里的魔数扫描结果充当目录。

运行普通测试：

```sh
cargo test -p mhf-resource --target aarch64-apple-darwin --test container_decode
```

只读游戏实样验证：

```sh
MHF_RESOURCE_GAME_ROOT=/path/to/mhfzz cargo test -p mhf-resource \
  --target aarch64-apple-darwin --test container_decode -- --ignored --nocapture
```

目标三元组按宿主调整。忽略的实样测试必须显式指定游戏目录，缺失时直接失败，
不会把没有执行的实样检查报成通过。
