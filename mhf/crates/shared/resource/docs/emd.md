# mhfemd.bin

The parser accepts the **decoded, unrelocated** payload. ECD/JKR envelopes are
handled by the workbench's existing decode/repack path. There is no verified
magic; workbench recognition requires the filename `mhfemd.bin`.

## Verified layout

Derived from ZZ HD loader `10AFA180` and consumers `10050530` / `1086F270`:

| Location | Meaning |
| --- | --- |
| payload +0 | little-endian u32 offset to header |
| header +4 | u8 species-slot count |
| payload +12 | little-endian u32 offset to species records |
| species record | 192 bytes, indexed directly by species ID |

The loader accesses 24 root DWORDs (96 bytes); consumers read header fields
through +34 (u16), so the validated header prefix is 36 bytes. Offsets are
relative to the decoded payload, not the record or envelope. Initial parsing
validates this prefix and the species-record array; other tables are validated
on demand. A malformed optional view does not prevent inspecting other tables.
Nonempty tables must not overlap the root/header or exceed the payload.

### Root directories and tables

`N` below is the resource's species-slot count. For directly species-indexed
tables, the view covers these N slots, not a claimed physical allocation size.
Header counts are little-endian u16 unless explicitly marked u8.

| Root index | Shape / count | Established use |
| --- | --- | --- |
| 0 | 36-byte header prefix | Counts; other bytes remain raw |
| 1 | 12 offsets → N × 34 bytes | Nine signed part initial values at +0..+16; byte +18 initializes actor+2924 |
| 2 | N × 52 bytes | Fixed species parameters, including part recovery ratio at +32 and request timer input at +44 |
| 3 | N × 192 bytes | Species records |
| 4 | 12 offsets → N × 80 bytes | Configuration-selected parameters; individual fields remain raw |
| 5 | N × 6 bytes | Byte +4 used as display classification |
| 6 | 3 offsets | Target lengths and semantics unresolved |
| 7 | header+12 × 12 bytes | Species lookup: +0 species byte, +4 probability-data offset, +8 health multiplier |
| 8 | unresolved extent | Float array indexed by actor+3212 in health calculation (`10859100`) |
| 9 | header+16 offsets | Target extents unresolved |
| 10 | zero-terminated offsets → N × 90 bytes | Parameter profiles; terminator is preserved |
| 11 | N × 18 bytes | Nine signed part-index mappings |
| 12 | unresolved | Offset preserved; no target extent inferred |
| 13 | header+18 × 28 bytes | Species / signed selector matching and six float multipliers; negative selector is wildcard |
| 14 | header+20 × 12 bytes | Three-key lookup and float multiplier |
| 15 | header+22 × 2 bytes | Counts for corresponding root-16 groups |
| 16 | header+22 offsets | Each group has root-15[index] × 32 bytes |
| 17 | header+24 × 8 bytes | Species byte, actor+3389 key, selector byte, target offset |
| 18 | u8(header+26) × 18 bytes | Eight word values and species byte at +16; byte +27 is not part of count |
| 19 | header+28 × 32 bytes | Species-associated anchor parameters; +26/+28 select counted four-byte action rules |
| 20 | unresolved | Offset preserved; no target extent inferred |
| 21 | N × 2 bytes | Species-indexed classification key |
| 22 | header+34 × 28 bytes | Species-keyed float multipliers, including health (+20) and part values (+24) |
| 23 | unresolved | Offset preserved; no target extent inferred |

Root offsets and most directory entries are relocated **unconditionally** by
`10AFA180`; zero is not a universal null pointer. A nonempty known table pointing
into the root is rejected. The zero terminator of root 10 and nullable species
links are separate rules. Aliased targets remain aliased; they are not copied,
merged, or sized by subtracting neighboring root offsets.

### Species record links

Relocated links are +8, twelve DWORDs at +12, twelve at +72, +176 and +184.
The +8 link selects 24-byte scaling records using actor+3212. The +72 directory
selects anger parameters via `10AA1520`, not a declared list of variant types.
The +168 word array supplies base values in health calculation; +188 initializes
actor+3392. Other bytes remain raw unless a scalar access is established.

`111A5080` reads +176 using `40 * (parameter_id + 200 * bank)`, with IDs below
200. The bank count is not established, so the inspector does not infer the
whole target extent. At +184, the loader processes 200 eight-byte entries,
relocating the first DWORD only when both DWORDs are nonzero; the second DWORD's
full meaning and nested target layout remain unresolved.

`Emd::directory_table(3, species_id)` exposes this fixed-size +184 directory
(or `None` for a zero link). The workbench shows it beneath each species and
lazily expands its 200 records as `target_offset: u32` and `value_04: u32`.
It preserves shared directory offsets and does not interpret `value_04` as a
count, validate unknown target extents, or follow the nested links. Invalid
directory extents are reported independently of the species scalar fields.

### Root-19 anchors and action rules

`10030740` selects a root-19 record by the u16 species key at +2, falling back
to the first record when no key matches. Its call at `10030830` passes that
record in EDX and the actor in ECX to `113CAEA0`. The latter reads:

- +4: signed bone index; zero becomes 1, values >=118 become 0. It uses the
  actor transform when a bone matrix is unavailable. Negative values are not
  clamped in this routine; the inspector preserves the signed value.
- +6/+8/+10: signed X/Y/Z local offsets, transformed by the selected matrix
  and added to its translation to obtain the anchor position.

`113CB2E0` stores root 19 in the runtime context; `113CB370` binds per-actor
records. `113CBB20` reads the selected record's u16 count at +26 and pointer
at +28, then scans exactly that many four-byte entries:

| Offset | Type | Meaning |
| --- | --- | --- |
| +0 | u8 | Boolean result interpreted as nonzero, not restricted to 0/1 |
| +1 | u8 | Exact match against actor+21 (action group) |
| +2 | u16 | Exact match against zero-extended actor+20 (action ID) |

The first matching entry returns whether +0 is nonzero and sets/clears bit
0x10 of the caller's per-actor control record inversely. No match returns the
caller's supplied default; the bit has already been cleared on entry. These
rules are not identified as AI transitions or action enable/disable switches.
Values above 255 in the encoded action ID cannot match the byte field, but are
preserved without normalization. The loader relocates +28 only if both count
and offset are nonzero, so inactive links are not dereferenced or validated as
active target arrays. No extent is inferred from a neighboring record.

### Evidence and coverage

Key native consumers: `10846950` / `108474F0` (root 1), `10847410` / `10856B80`
(root 2), `10859100` (root 7/8 and species scaling/health), `1085F680` and adjacent
helpers (root 13), `1085F920` (root 14), `1006E310` (root 15/16), `10AFA530`
(root 17), `10AFA590` (root 18), `10851990` / `108474F0` (root 22).

This is **not a complete semantic decode**. Unresolved roots, nested target
lengths, unknown fields inside fixed-stride records, and bank counts still need
consumer tracing and real-asset verification. In particular, unused-looking
roots are not declared empty merely because no consumer has been found.

The workbench exposes known tables and directory targets lazily. Confirmed
scalar accesses are editable; other spans remain raw. Data stays in the original
backing buffer and fixed-size edits use normal envelope repacking. No variant
support flags are inferred, and variant behavior is unchanged.

Enumeration uses the header count, including zero/placeholder slots. A slot is
not proof of a spawnable monster or an available AI descriptor. Chinese labels
in `species.rs` are display metadata only; unnamed IDs use `emNNN`. Counts above
the known name range remain visible.

## Other species-ID constraints

- Debug catalog: candidates now come from the loaded, relocated EMD count. The
  native dispatcher at `118C3628` still filters spawn support and is bounded to
  the verified 177 entries; expanding EMD alone must not read past that table.
- `mods/quest/src/provider/binary.rs`: spawn/retarget validation rejects 0 and
  IDs >=177. Retained as native creation compatibility checks, not enumeration.
- `mods/monster/src/ai/overlay.rs`: `SPECIES_LIMIT = 0x83` reflects supported static
  AI descriptor lookup; IDs above it need dynamic resolver support.
- `mods/monster/src/species.rs`: patches eight native limits from 177 to 255;
  this does not populate new dispatcher records or register species.
- Debug action masks and variant lists remain native-behavior mappings, not a
  resource roster. Variant handling is unchanged.

## Real-resource verification

Local resources were found under `~/Games/mhfz/dat`, `~/Games/mhfzz/dat`, and
`~/Games/zz/mhfz/dat`; the first and third files are byte-identical. The DLL in
`mhfz` has SHA-256 `95c580195f4080d2e9582c8c9df36abeb280476e088b6366583c5f138da8f301`,
matching the analyzed DLL. Do not assume the differently named installation
uses the same executable: `mhfzz` has a different DLL hash.

| Sample | Encoded SHA-256 | Decoded bytes | Species slots | Root 22 |
| --- | --- | --- | --- | --- |
| mhfz | `fc1cc983f1b3411fb62f256eb40f31984da3a160747d37cd8cf80a26d57d8c81` | 2,572,800 | 177 | offset 2,571,008; 64 records |
| mhfzz | `e68e03f1274d9d055cc8e42b0427bb99b0cebfe9c905f951900b991f4cc8afd1` | 2,571,008 | 177 | offset 35,968; 40 records |

Both samples pass all 20 known root views, every supported nested directory,
and workbench scalar-edit/envelope-repack/reopen checks. The latter compares
the entire decoded payload against the original with only the requested edit.
Tests read local resources without modifying the installed files.

Both contain 83 species with nonnull +184 links, referencing 82 distinct
200-entry directories. Species 58 and 144 share an all-zero directory; species
1 has no +184 directory. Across distinct directories, 3,755 entries have both
DWORDs nonzero and reference 3,747 distinct target offsets.

### +184 directory: measured shape

The loader loop at `10AFA2FA` walks 200 eight-byte entries. For each entry it
reads the DWORD at +0 and the DWORD at +4, and relocates only the DWORD at +0,
and only when both are nonzero. The DWORD at +4 is never relocated, so it is
plain data there, and the loader only tests it for zero. It does not multiply it
by any stride and does not traverse the targets, so the loader alone cannot
establish it as a record count.

Reading each non-null target as `value_04` records of 16 bytes is the only
stride among 4, 8, 16 and 32 that fits the whole `mhfz` sample: 3,755 non-null
entries give 39,682 records, in which bytes +3, +9, +11, +13 and +15 are zero in
every single record, and 3,747 distinct target spans overlap the next target in
only 8 cases. Under the 4- and 8-byte readings those zero lanes fail and the
records stop fitting the span.

Measured u16 lanes over those 39,682 records:

| Offset | Range | Distinct | Notes |
| --- | --- | --- | --- |
| +0 | 0..2,047 | 660 | byte +1 is 0..7 |
| +2 | 0..115 | 94 | |
| +4 | ..2,074 | 72 | byte +5 is 0, 1, 2, 6 or 8 |
| +6 | 0, 1, 50, 256, 257, 306 | 6 | byte +7 is 0 or 1 |
| +8 | 0..63 | 18 | byte +9 is always 0 |
| +10 | 0..63 | 23 | byte +11 is always 0 |
| +12 | 0, 1 | 2 | byte +13 is always 0 |
| +14 | 0, 1 | 2 | byte +15 is always 0 |

Stored nonzero counts range from 1 to 74, and no shared target carries
conflicting counts. Within a list, +0 is nondecreasing in only 814 of 3,755
lists, and 155 records carry an odd +0 value, so +0 is not established as a
frame or time. The two 0/1 lanes and the six-value lane look enumeration-like,
but no field here has been tied to a named native consumer, and the parser and
workbench keep the opaque link fields instead of enforcing this layout.

### Shared index space with +176 (evidence, not naming)

`sub_10050530` rejects an index of 200 or more, then reads
`parameters_176[40 * index + 8000]` and returns the DWORD at record +16. The
+176 target therefore holds 400 forty-byte records, and the validated index
range is 0..199 - the same cardinality as the +184 directory.

Measured on the sample: over the 126 species carrying data in either table, the
populated +184 indices are a strict subset of the populated +176 indices, and
the only index populated in +176 but never in +184 is 0. This links both tables
to one index space without establishing what selects that index.

### Static consumer audit

For the matching `mhfz` DLL, the current IDA database reports 2,185 direct
references to the decoded EMD global at `1E77DCE0`, in 1,455 functions. Full
disassembly and pseudocode were collected for those functions, including the
13 decompilations whose inline MCP results were truncated. This closes the
previous direct-reference collection gap, not every possible alias or indirect
call path.

The audit has not identified a +184 target consumer. Concrete exclusions are:

- `111A5010` / `111A5080` access the +176 parameter table, not +184.
- Some apparent EMD arguments are residual registers in inferred prototypes.
  For example, `101CFD00` uses its quest-ID input in DX, not the apparent ECX
  EMD argument propagated by callers.
- Apparent pointer returns require checking their callers before treating them
  as resource accessors. `10851C60`, for example, can leave the EMD base in EAX
  on an early exit, but its other paths return unrelated values. Three callers
  (`11041350`, `110F5DB0`, `1119A2F0`) eventually call `10850D70` after replacing
  AL; that callee reads AL as a small mode value, not EAX as a resource pointer.
- The return chain `1094E5F0 -> 10950440 -> 10952F30` does not pass an EMD pointer
  into `1095F670`: the latter overwrites EAX at `1095F671` before using it.
- `1197E980` holds `10E20C90`, at +0xAC4 in the `CEnemy141_Routine` vtable
  rooted at `1197DEBC` (installed by constructor `10E1E890`). The indirect call
  at `10E2048A` is followed by `mov eax, [ebx+4]` at `10E2048C`, not a
  dereference of the callback's residual EMD return. The generic forwarding
  site at `100513C2` returns without replacing EAX, but the species-141 vtable
  overrides its +0xA30 slot with `10E203E0` and does not contain `10051310`.
  The IDB also reports no direct code callers of `10051310`. That inherited
  base-method path therefore does not propagate this species-141 callback's
  EMD value through the ordinary, unmodified vtable.
- Filename `1199E0D8` has one direct data reference, `11A460DC`; that pointer's
  sole direct code reader is loader `10AFA180`. This does not establish that a
  separately constructed filename or a generic resource cache is impossible.

Return propagation and indirect callback/table paths are not all closed. This
is therefore neither a proof of non-use nor a verified target schema; the
candidate 16-byte layout and its business meaning remain unconfirmed.

### Other observations and checks

Unresolved root offsets in both samples are 8=160, 12=67,872, 20=37,472,
23=35,936. Root 8 starts with floats 0.3, 0.3, 0.35, 0.4; root 12 starts with
0.01 floats. These observations do not establish array lengths. In particular,
the different root-22 placement must not be used to infer root-23 size.

Reproduce with `MHF_EMD_PATH=/path/to/dat/mhfemd.bin`:

```sh
cargo test -p mhf-resource --target aarch64-apple-darwin --test emd_real -- --ignored --nocapture
cargo test -p mhf-workbench --target aarch64-apple-darwin real_emd_workbench -- --ignored
```

Synthetic boundary tests remain separate. Complete nested semantics and
in-game UI validation are still outstanding.
