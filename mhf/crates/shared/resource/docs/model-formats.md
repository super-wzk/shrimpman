# FMOD / FSKL file representation

These parsers describe unwrapped file data, not the client's relocated objects,
Direct3D buffers, or the 32-bit index extension in `mhf.geometry`. They read
bounded source slices with standard little-endian conversions.
The complete source slice remains authoritative: block order, duplicate kinds,
unknown payload, counted-record tails, and file tails are all retained.

Decoded `f32` values undergo no arithmetic. Positions are not rescaled; UV V is
not flipped; normals and quaternions are not normalized; vertex colors remain
file floats (often 255); weights remain percentages (often 100). No decoded
node ID is substituted for a file ordinal. `as_bytes()` returns the exact input.
The explicit position/translation edit methods copy the input and patch only
the selected fixed-width field; changing a public decoded value by itself is
not a serialization operation.

## Evidence

Read-only analysis of the locally loaded `mhfo-hd.dll` in IDA established:

| Function | Evidence used |
| --- | --- |
| `10002680`, `10002630`, `10002AF0` | FILE/MAIN/OBJECT traversal: three little-endian u32 header words, 12-byte header, size includes header, MAIN object selection by ordinal |
| `10002960` | OBJECT `0x50000` material-list count |
| `10003790` | Positions/normals are 3 floats, UVs 2 floats, colors 4 floats; `0xC0000` stores a per-vertex u32 count followed by u32 bone index/f32 weight pairs; runtime conversion truncates indices and quantizes weights, so file types must remain wider |
| `10002A10`, `10003790` | Triangle-strip length is `packed & 0x7fffffff`, indices are u32, bit 31 selects winding, groups `0x30000` and `0x40000` remain distinct |
| `10003190`, `10003790` | `0x120000` supplies four floats per vertex; shader meaning remains unconfirmed |
| `100024F0` | `0x100000` is a u32 bone map on disk, converted to WORDs at runtime |
| `10002560` | `0xF0000` has a versioned 18-word structure; `RenderingBlock::words` preserves all 18 u32 values without guessing individual render-state meanings |
| `10002740`, `100028F0` | Material table kind 9 and texture table kind 10 contain individually headed records and advance using each record's size |
| `100027B0` | Material payload `0x00/0x10/0x20` each contain four floats; `0x30` is read as float and converted to integer; `0x34` is texture count; texture table indices begin at `0x100`; image ID is texture payload word 0 |
| `100021C0`, `10002220`, `100022A0` | FSKL metadata supplies root node ordinals; node enumeration skips blocks whose kind's low byte is zero; child/sibling links use node ordinals; record `0x10/0x20/0x30` contain four floats each; `0x40/0x44` supply the low WORDs at compact node `+10/+2` |
| `10009DD0` | Compact node `+10` is sign-extended from i16 into runtime node `+368`; compact node `+2` becomes the WORD motion tag at runtime node `+198` |
| `10008D30`, `10008CC0`, `10009140`, `10008670` | Runtime node `+198` selects nodes for motion lookup/binding, curve sampling, and pose blending |

The public format research was cross-checked at
[`Houmgaor/MHFrontier-Blender-Addon` revision `29b23a1269e323b7e5ec6b79cd3cf71743800784`](https://github.com/Houmgaor/MHFrontier-Blender-Addon/tree/29b23a1269e323b7e5ec6b79cd3cf71743800784),
especially `mhfrontier/fmod/fblock.py`, `common/standard_structures.py`, and
`fmod/fmesh.py`. It supplies the scale/rotation/translation naming and texture
dimensions. Those names do not imply that every render or animation semantic
has been established. No importer/exporter code is incorporated.

The native loader and actual file bytes take precedence where that research's
export code or descriptions disagree:

- A bone payload is **256 bytes**, not the 252 stated in its exporter comment.
- FMOD tables have nested per-record headers. A kind 1 below material table 9
  is a material record, not another FILE container. Generic recursion by kind
  alone would misread its first color as a child block header.
- Material payload `0x2C` is the fourth component of the third color vector,
  not a `u32 materialFlags` field. Native code reads it as a float.
- Weight 100.0 is not converted to 1.0 during parsing.
- The root-index table is not arbitrary zero padding. Node `0x44` is an
  animation-group tag; no IK-chain interpretation is established.

## FMOD structure

All blocks have `{kind: u32, count: u32, size: u32}` encoded little-endian.
`count` describes children or records according to the parent context. Unknown
kinds do not inherit a guessed element width. The top-level kind is 1, not an
ASCII `FMOD` prefix.

```text
FILE 1
  INIT 0x20000                 count × u32
  MAIN 2
    OBJECT 4
      FACE 5
        STRIPS_A 0x30000       count × (packed_count:u32, indices:u32[])
        STRIPS_B 0x40000       same layout, separate native variant
      MATERIAL_LIST 0x50000    count × u32
      MATERIAL_MAP 0x60000     count × u32, per strip
      POSITIONS 0x70000        count × [f32;3]
      NORMALS 0x80000          count × [f32;3]
      UVS 0xA0000              count × [f32;2]
      COLORS 0xB0000           count × [f32;4]
      WEIGHTS 0xC0000          count × (influence_count:u32, [bone:u32, weight:f32][])
      WORD_GROUPS 0xE0000      count × (word_count:u32, words:u32[]), semantic unknown
      RENDERING 0xF0000        count 1: 18 original u32 words, semantic unknown
      BONE_MAP 0x100000        count × u32
      ATTRIBUTE_12 0x120000    count × [f32;4], semantic unknown
      unknown blocks
  MATERIALS 9
    record kind 1, count 1    256-byte prefix + texture_count × u32
  TEXTURES 10
    record kind 0, count 1    256-byte payload
  unknown sections
```

Material payload:

| Offset | Rust representation |
| --- | --- |
| `0x00`, `0x10`, `0x20` | Three separate `[f32; 4]` color parameters; shader role intentionally not guessed |
| `0x30` | `f32` parameter |
| `0x34` | u32 texture-reference count |
| `0x38..0x100` | 200 original unknown bytes |
| `0x100..` | Texture table ordinals, u32, in file order |

Texture payload `0x00/0x04/0x08` is image ID/width/height as u32;
`0x0C..0x100` retains 244 unknown bytes. An image ID refers to the accompanying
texture bundle; it is not a pointer and is not necessarily the material's
texture-table ordinal. Unknown record kind/count variants remain explicit
unknown table entries instead of being decoded under an assumed layout.

`Object::validate_geometry()` separately checks known attribute counts and
triangle-strip index bounds. Structural parsing deliberately permits
degenerate strips, unknown variants, and invalid semantic references so the
workbench can inspect them without silently repairing the source.

The rendering block has a 72-byte payload in all 92 occurrences found in the
selected `em077_b.pac` and `em077_b-hd.pac` samples (46 per file). Both files
contain five occurrences in model entry `0/0` and 41 in entry `3/0`. Their first
word is `0x00010000`; it remains part of the raw word array. Extra payload bytes
are retained in `trailing`, and record counts other than 1 remain unknown.

The `0xE0000` block's grouped-word structure is verified from actual file bytes.
The inspected native OBJECT paths `10002AF0` and `10003790` do not consume this
block, so `WordGroupsBlock` claims only a structural layout. It does not identify
the words as bone, face, or material indices, or interpret `0xFFFFFFFF` as a
sentinel. Each group retains its count-word file offset and every original u32;
the block count supplies the number of groups, and trailing bytes are retained.

In the selected ordinary monster packages, `em001.pac` has two blocks with group
sizes `[32]` and `[22]`; `em002.pac` has four with `[20]`, `[28, 33]`, `[28, 20]`,
and `[33]`; `em007.pac` has four with `[22, 10, 22, 17, 21]`, `[21]`, `[12]`, and
`[17, 10, 22, 22, 12]`. All ten blocks are exactly consumed by this layout and
their 424 words are `0xFFFFFFFF`. `em001_b.pac` has no such block.

The follow-up audit covered all 28 files reported with unknown FMOD blocks in
the local 581-monster-package scan. All 67 blocks decoded as `WordGroupsBlock`,
containing 183 groups and 13,518 original words, all `0xFFFFFFFF`. Every block
was exactly consumed with no trailing bytes, and no unknown components remained
in those files' FMOD resources. This confirms structural coverage only.

## FSKL structure

Root kind `0xC0000000` contains root-index and node blocks. Root table kind 0
holds `count × u32` node ordinals. Known node block kinds `0x40000001`,
`0x40000002`, and `0x40000003`, count 1, have the following 256-byte payload:

| Offset | Rust representation |
| --- | --- |
| `0x00` | `node_id: i32` |
| `0x04/0x08/0x0C` | Parent/first-child/next-sibling ordinal, i32; -1 means no link |
| `0x10` | Scale `[f32;4]` |
| `0x20` | Rotation `[f32;4]` |
| `0x30` | Translation `[f32;4]` |
| `0x40` | `unknown_40: u32`; runtime sign-extends its low WORD as i16; subsequent use unconfirmed |
| `0x44` | `motion_tag: u32`; low WORD groups nodes for animation binding, sampling, and pose blending |
| `0x48..0x100` | 184 original unknown bytes |

### Native node metadata

Both metadata fields retain their complete on-disk DWORD. `100022A0` writes
the low WORD of payload `0x40` to compact node `+10`, and payload `0x44` to
compact node `+2`. `10009DD0` sign-extends the former into runtime node `+368`
and copies the latter into the WORD at runtime node `+198`. Consequently,
`unknown_40 = 0x4321FFFF` is consumed as `-1`, while
`motion_tag = 0x76541234` is consumed as tag `0x1234`; neither conversion
discards the original high bits in the file parser. The workbench shows the
native value alongside the original DWORD.

`10008D30` finds the first matching motion tag in child-before-sibling order.
`10008CC0` consumes a track and descends into children only for matching nodes,
while still visiting siblings. `10009140` similarly limits curve sampling by
tag, and `100085C0`/`10008670` select and blend tagged groups between poses.
This establishes an animation-group tag, not a bone ID, MOT directory index,
IK-chain ID, or a universal body-part category. In both local em001 packages,
tags 0, 1, 2, and 3 cover nodes 0–28, 29–39, 40–44, and 45–47 respectively;
populated directory records 0, 2, and 4 contain 29-, 11-, and 5-track clips.

The business meaning of `unknown_40` remains unconfirmed. A read-only audit of
47 local monster, NPC, weapon, and stage files found 64 paired FMOD/FSKL sets.
In 37 sets all FMOD objects have weights and every `unknown_40` is `0xFFFFFFFF`.
The other 27 sets have no object weights, an extra root with `0xFFFFFFFF`, and
one node per object with values 0 through `object_count - 1`. For example,
em001's second model has 17 objects and 18 nodes. This supports an object-index
hypothesis but does not establish a native consumer or sentinel behavior.
The audited loading, animation, and matrix-upload paths do not resolve that
meaning, so the field retains its unknown name; `-1` is displayed as a signed
value without labeling it as a missing mesh.

The second skeleton (`3/1`) in both `em150.pac` and `em150-hd.pac` has root
indices `[0, 212]`. Node 212 at `0xDE10` has kind `0x40000003`, count 1, and a
256-byte payload. Its ID is 212, all three links are -1, and its stored scale,
rotation, and translation occupy the same offsets as other nodes. This variant
is decoded as the common node representation while retaining its original kind;
no light, camera, or animation role is inferred from the variant number.

Nodes are never sorted by `node_id`. Unknown nodes keep their ordinal slot.
All metadata blocks remain available through `blocks`; blocks with low-byte
kind zero do not take a node slot. Recognized root tables remain available
individually; `root_indices()` returns the first recognized table.
`validate_hierarchy()` checks known link ranges and child/sibling cycles with
an iterative traversal. It reports unsupported node layouts instead of
claiming to validate them. The parser does not synthesize bind matrices,
normalize rotations, or create physics/IK interpretations.

## Verification

Synthetic tests cover raw float bit patterns (NaN and signed zero), colors and
non-normalized weights, 32-bit bone/vertex references, strip winding, nested
material/texture records, arbitrary unknown data, truncated headers/records,
huge counts, per-field error offsets, out-of-range links and cycles. Position
and translation edits compare **every byte outside the changed field** and
reparse the result.

External game samples are optional, supplied through environment variables;
no game data is committed:

```sh
MHF_RESOURCE_FMOD_SAMPLE=/path/to/unwrapped.fmod \
MHF_RESOURCE_FSKL_SAMPLE=/path/to/unwrapped.fskl \
cargo test -p mhf-resource --target aarch64-apple-darwin external_
```

Verified on 2026-09-11 against unwrapped local `m_editpl` resources:

| Sample | Bytes | Observed structure | SHA-256 |
| --- | ---: | --- | --- |
| FMOD | 4,824,388 | 77 objects, 64,316 positions, 176 material and 176 texture records | `ad4ba793c072ac2a9def2877152ceea20d5959dfe36305380ba325911306d630` |
| FSKL | 5,656 | 21 bones and a root-index table | `d79b5c5ff11ef2b75d96dd0268f1cc9b11de31dd2b62400d913ceaa759c8d835` |

Both parse and pass the respective geometry/hierarchy validation. These are
file-format tests, not proof of in-game preview, shader parity, physics, or
arbitrary edited assets being accepted by the client.
