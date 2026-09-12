# MOT and equipment-effect records

The parser describes file data from the ZZ HD client. Addresses below are the
preferred-image virtual addresses in `mhfo-hd.dll`, not offsets inside a resource.
No game assets are included in this crate.

## MOT evidence and boundaries

`108FD1D0` unwraps JKR when necessary and receives the motion-directory group
count from its caller. Each group is an eight-byte `(u32 count, u32 table_offset)`
record. The table contains `u32` motion offsets relative to the decoded archive;
`0xffffffff` is an absent motion. The native lookup divides `table_offset` by four
before addressing DWORDs. `MotionArchive` preserves the original field including
its low two bits, holes, aliases and original group/slot ordering. It does not
infer a group count from a filename or from a guessed magic value. Native action
IDs are resolved using `group = id / 100`, `slot = id % 100`.

The group count is demonstrably not a format-wide constant: `1089F700` invokes
the loader with 10, 2 and 6 groups for different resources; `108FD350` supplies 6.
A trailing empty directory-shaped record does not establish that the native
caller consumes another group. A resource browser must distinguish observed file
records from the caller-supplied group count, rather than treating every MOT as
having three or six groups.

Both ordinary and HD monster packages use member 2 for motions. `108FBA70` first
tries `emmodel-hd` through `108E2850`, falls back to `emmodel` through `108E27D0`,
then sends either member through `108FD380`. That function supplies four groups
for monster IDs 51, 111, 162, 164, 165 and 166; otherwise it supplies twice the byte
at `mhfemd` table 2, record size 52, offset 5. IDs at least 177 use record 1.
The additional-monster path `108FC420` has the same HD/ordinary fallback, but
`108FD470` always uses that table-derived count without the six-ID exception.
`10AFA180` loads `mhfemd.bin` into `1E77DCE0` and relocates its DWORD at +8 to that
record table. These are caller contracts, not mandatory dependencies for a file
browser, and neither the filename suffix nor the number of outer PAC entries
encodes a universal motion group count.

`ObservedMotionDirectory::probe_with_budget` identifies a candidate record region
without using filenames, monster IDs or DAT data. The first table reference must
bound an aligned record region, and every record, complete offset table and
distinct populated motion must then validate. Tables must follow the records;
motions must follow the tables and must not overlap distinct motions. The probe
requires at least one declared slot; every populated motion must have native
kind 1 or 2. Complete tables containing only `0xffffffff` remain valid empty
directories, while zero-count records without any slots are insufficient to
identify the format. It preserves
empty records, empty slots, table aliases and motion aliases, and exposes the
result as `directory` plus `record_count()`. This record count is deliberately
not called a native group count. Ambiguous or unsupported layouts can still be
parsed through `MotionArchive::parse_with_budget` with an explicit caller count.
The probe's budget independently caps both records and aggregate decoded slots;
aliased motions are validated only once.

`100018B0` and allocation-size calculation `10001790` establish the motion header:

| Offset | Stored type | Meaning |
|---|---|---|
| `00` | u32 | Original motion kind/flags; low byte 1/2 selects native track type 1000/2000 |
| `04` | u32 | Track count |
| `08` | u32 | Byte length including the header |
| `0c` | u32 | Tested for nonzero by the native loader; full meaning unconfirmed |
| `10` | u32 | Copied when the preceding field is nonzero; full meaning unconfirmed |
| `14` | blocks | First track, including empty tracks |

Every track has a twelve-byte `(u32 kind, u32 channel_count, u32 byte_length)`
header. Every channel has the same header shape but its count describes keys.
`10001EE0` reads channels in order, advances by each channel's byte length, and
maps single low-nine-bit masks `1, 2, ..., 100h` to native target slots `0..8`.
Neither a track's mask nor the sequence of nonempty tracks is interpreted as a
bone ID. Empty leading tracks must remain present. No non-root position channels
are removed.

The source encoding is the channel-kind byte at offset 2. A clear high bit causes
native channel conversion to be skipped. The six known encodings are copied by
`10001B80`, `10001BF0`, `10001C80`, `10001D20`, `10001DA0`, `10001E40`:

| Encoding | Stride | Rust storage variant | Frame offset/type |
|---|---:|---|---|
| `11` | 4 | I16Pair | +2 / i16 |
| `12` | 8 | I16Quad | +2 / i16 |
| `13` | 12 | Mixed12 | +6 / i16 |
| `21` | 8 | F32Pair | +4 / f32 bits |
| `22` | 16 | F32Quad | +4 / f32 bits |
| `23` | 20 | F32Five | +8 / f32 bits |

Frame positions and signedness are confirmed by `100017F0` (first frame) and
`100018B0` (last frame). The native key-copy routines use the low 16 bits of the
count; the full DWORD remains available. Unknown encodings, flags, interpolation
parameters and block tails remain raw, and float values are stored as `u32` bits.
This API does not assert an interpolation formula, frame rate, axis convention,
fixed-point scale, bind-pose rule or bone mapping. Those require verification of
the subsequent native evaluator, rather than a Blender importer's assumptions.

The parser uses `std::io::Cursor<&[u8]>` for sequential headers; validated fixed
records use standard slices and `from_le_bytes`. Known key sizes and parent
boundaries are checked before decoding. `Motion::with_key` changes one existing
key without changing its encoding or byte count; it copies every other byte.

`MotionArchive::parse` limits the aggregate decoded slot count to one slot per
four source bytes, including repeated slots in aliased or overlapping tables.
This bounds their allocation and traversal even when each individual table fits
inside the resource. `parse_with_budget` accepts an explicit aggregate slot cap
for heavily aliased directories; holes and aliases still retain their positions
and the source bytes remain unchanged.

### Verification

Synthetic tests cover all six key storage layouts, malformed boundaries, aliases,
empty slots and bit-preserving patches. Round-trip checks compare each encoded
key with its original bytes; replacing a key with itself must preserve the
complete motion, including unknown tails.

A complete directory whose slots all contain `0xffffffff` is a valid empty motion
resource. Member 2 of `emmodel/em171.pac` and `emmodel-hd/em171-hd.pac` provides
such a case: all declared slots mark motions absent, and an empty trailing
directory record remains present. Compressed size alone cannot distinguish an
empty directory from a malformed resource.

The opt-in `motion_native_samples` tests use `MHF_MOTION_SAMPLE_DIR` for decoded
samples: the key round-trip test reads `npc41-decoded.mot` and `em019-chunk-2.bin`,
while the empty-directory test reads `em171-decoded.mot` and
`em171-hd-decoded.mot`. These tests also assert the expected fixture structures.
A separate test uses `MHF_RESOURCE_GAME_ROOT` to read standalone clips from
`dat/mytra.bin`. Run from the `mhf` workspace and select the test matching the
provided inputs, with `<host-target>` replaced by the host triple from `rustc -vV`:

```sh
MHF_MOTION_SAMPLE_DIR="<decoded-motion-directory>" \
cargo test -p mhf-resource --target "<host-target>" --test motion_native_samples \
  decoded_client_motion_samples_keep_every_track_and_encoded_key -- --ignored
```

These tests validate file structure and encoded values. Native animation binding,
interpolation and in-game playback require separate verification.

## Equipment effects are four distinct tables

DAT indices refer to the game's DAT pointer table after loading, not standalone
file offsets. Callers must resolve those file-table locations and extents first.
The record parsers do not assume one shared layout or attach an effect at runtime.

| DAT entry | Rust type | Record size | Confirmed fields |
|---|---|---:|---|
| 160 | AttachmentGroup | 18 | +0 u16 part code; +2 eight u16 definition IDs |
| 161 | AttachmentDefinition | 128 | +0/+4/+8 local XYZ float bits; +0d node index; +0e attachment-mode byte |
| 165 | ModelEffectBinding | 24 | +0 part code; +2 weapon class; +4 variant selector; +6 model ID; +8 eight definition IDs |
| 166 | ModelEffectDefinition | 180 | +0/+4/+8 node translation delta bits; +0d draw group; +0e group entry; +0f node index; +10 u16 start delay |

`10BB2AB0` consumes entry 161 and resolves its node through a 448-byte node array.
`10BBA300` matches entry 165 against the equipment/model selection. `10BBAC20`
indexes entry 166 with stride 180 and initializes at most eight 148-byte runtime
slots for the target part. `10BBE150` and `10BBF340` then check draw group/entry and
resolve entry 166's node, also with stride 448. An entry-161 node index is at 0d;
an entry-166 node index is at 0f. These must not be conflated.

Zero terminates each definition-ID list during native loading. The parser keeps
the unused slots after that zero. Entry 160 and entry 165 part codes also retain
their separate native namespaces. The model-effect loader clears the runtime
arrays once, then each matching record writes from slot zero; removing all model
checks would overwrite earlier slots and can retain their tails. Parsing these
tables therefore does not imply that arbitrary effects can simply be combined.

Public record fields deliberately expose unknown bytes and raw float bits. Parsing
and `to_bytes` cover every byte. Only explicitly changed fields alter the serialized
record; no reserved fields are zero-filled and no bit patterns are normalized.

DAT 161/166 的动画尾部已拆分为独立字段，包括资源序列、旋转、缩放、颜色、
透明度、UV 和状态控制；附着特效另含视线偏移和拖尾参数。详细偏移、原生消费证据
和仍未确认的范围见 [equipment-effect-animation.md](equipment-effect-animation.md)。
