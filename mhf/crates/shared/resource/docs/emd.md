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
| 1 | 12 offsets → N × 34 bytes | Nine signed part-durability bases at +0..+16; byte +18 selects part-damage handling |
| 2 | N × 52 bytes | Fixed species parameters, including animation-channel count at +5, part recovery ratio at +32 and tracking/alert retention at +44 |
| 3 | N × 192 bytes | Species records |
| 4 | 12 offsets → N × 80 bytes | Configuration-selected initial values, increments, limits and flags; only confirmed scalar fields are typed |
| 5 | N × 6 bytes | Byte +4 used as display classification |
| 6 | 3 offsets → signed byte weight/value pairs | Lists end at a single `FF` weight byte; each sentinel is preserved separately |
| 7 | header+12 × 12 bytes | Species lookup: +0 species byte, +4 offset to five four-byte probability rows, +8 health multiplier |
| 8 | unresolved extent | Float array indexed by actor+3212 in health calculation (`10859100`) |
| 9 | header+16 offsets → AI scripts | Script boundaries use the shared bounded bytecode reader; directory indices are not DLL script-table indices |
| 10 | zero-terminated offsets → N × 90 bytes | Nine part thresholds, increments and response kinds; other bytes remain raw; directory terminator is preserved |
| 11 | N × 18 bytes | Nine signed part-index mappings |
| 12 | N × 36 bytes | Nine F32 part-health ratios per species |
| 13 | header+18 × 28 bytes | Species / signed selector matching and six float multipliers; negative selector is wildcard |
| 14 | header+20 × 12 bytes | Three-key lookup and float multiplier |
| 15 | header+22 × 2 bytes | Counts for corresponding root-16 groups |
| 16 | header+22 offsets | Each group has root-15[index] × 32 bytes |
| 17 | header+24 × 8 bytes → AI scripts | Species byte, actor+3389 key, selector byte, script offset at +4 |
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

### Workbench navigation

Species entries sit directly beneath the EMD file node, followed by
**全局数据** (global data). Each species expands on demand and collects its
associated records:

| Association | Species view |
| --- | --- |
| Direct species indexing | The root-3 species record; roots 2/5/21 basic parameters and roots 11/12 part parameters |
| Configuration, then species | Roots 1/4/10, retaining each configuration index; root 10 keeps its variable directory count |
| Links in the species record | +176 parameter banks, +72 anger profiles, and the +184 parameter-link directory |
| Explicit species field | Roots 7/13/17/18/19/22, retaining conditions and original record indices |
| Grouped species field | Root-16 records with their root-15 group count, group index, original record index and actor+2394 key |
| Native script selection | Root-9 candidates whose species binding is established, including the Zinogre selections below |

Root-17 records expose their linked AI scripts within the species view.
Root 19 distinguishes explicit species matches from the first-record fallback;
the fallback remains a shared record. Root-16 associations retain their signed
species field and group context rather than treating the entire group as
species-owned. Global configuration indices retain editable directory pointers
without duplicating their species records. Root 15 retains each group's count
and the corresponding root-16 pointer, including empty groups.

Assigned records are listed under their species rather than repeated as full
global tables. Global data retains the shared root-6 weights, global roots
0/8/14/20/23, unassigned keyed records (including negative or out-of-range
species IDs), unassigned root-9 scripts, and the first root-19 record as the
shared default. Original table, configuration, group and record indices remain
visible. The file details retain all 24 root offsets; the full decoded buffer
remains accessible in the hex editor without a separate raw-directory tree.

These are views onto the same buffer ranges, not copied records or a file
layout conversion. Shared defaults and aliased targets keep the same backing
bytes through every entry point. EMD edit and selection identities validate the
record's kind, name and range relative to the EMD resource, so a changed
association list cannot redirect an old input to its next record. The UI does
not merge duplicate records, remove conditions, or select an active
configuration using game-instance state.

### Weighted and probability tables

`Emd::directory_table(6, index)` follows one of three root-6 offsets.
`10AB1A60` reads signed byte weight/value pairs until the weight is -1 (`FF`).
The sentinel owns only that first byte, including for an empty list; the reader
does not require an additional value byte. The inspector preserves signed
values and the sentinel, without normalizing weights or inventing a fixed
record count. `FF` in the value column is an ordinary value of -1, not a
terminator. The consumer accumulates weights, selects a value, applies species
record adjustments and writes spawn-record +54/+55; this does not establish
names for all encoded values.

`Emd::directory_table(7, index)` follows the offset at +4 of a species-lookup
record. `10AA2090` selects one of five four-byte rows, then reads a byte column.
The view therefore exposes five rows of four u8 thresholds, independently of
the number or placement of neighboring targets. The accumulated value in its
source record chooses a row using thresholds 30/60/120/180; a runtime selector
chooses the column. The test is `(random & 0x7F) < cell`, not a percentage
conversion. The native column guard is a signed-byte comparison with 4,
without an explicit lower-bound check. The inspector exposes the four stored
columns without inventing a gameplay name or normalizing their byte values.

Root 12 is a separate, directly species-indexed table of nine F32 part-health
ratios; its visible window is `N * 36` bytes. `10871950` at `10871C5D` reads
`root12[9 * species + root11[9 * species + part]]`; at `10871CC9` the value is
multiplied by decoded health before calling `10870060`. These entries are not
base health values or general damage multipliers.

### Part, fixed and status parameter profiles

Root-1 signed words at +0..+16 are part-durability bases, rather than the
monster's overall health. `10846950` at `1084698D` enables a part only when its
base is positive. `108480D0` obtains the adjusted value from `108474F0` and
reloads actor+840+8*part; `10842A20` subtracts part damage from those counters.
Nonpositive values retain their native meaning and must not be clamped by the
editor. Root-1 byte +18 is copied to actor+2924 at `108469BE`; `10842A20` uses
its values 1, 2 and the remaining branch to select part-damage handling. The
display labels are **部位耐久基数** and **部位伤害处理方式**; no names are assigned
to the individual handling values without further evidence.

Root-2 byte +5 supplies the animation-channel count: `1086F270` writes it to
actor+834, and `1084FE60` loops to that count, advancing animation slots and
calling `108FD910` for each channel. The display name **动画通道数** describes
that count, not a number of AI scripts or monster parts.

Root-2 +44 remains an **i16** despite the zero-extending load at `10858CAB`.
The subsequent `test ax, ax` / `jns` at `10858CC8` / `10858CCB` treats negative
values as zero, and the `jle` at `10858CD6` compares the signed word limit.
`10854B90` at `10854BDA` copies this field into per-player tracking timers at
actor+2688+2*player. `10854C70` refreshes them while detection is present and
otherwise decrements them before removing expired tracking bits. `10858C80`
also limits actor+2678 to the same value when no players remain tracked, then
exits alert mode when that signed countdown expires. The display name is
**追踪与警戒保持时间**; the internal `request_timer_limit` key is retained for
binding compatibility. These counts are not converted to seconds.

Root-2 +48 is a signed behavior-timer base. `1085AAD0` clamps a negative base
to zero, applies the species scaling-record multiplier at +20 and additional
runtime modifiers, and stores actor+3214. Target-kind 12 transports the value
in units of four: `1085B230` divides by four and `108542C0` multiplies its input
by four. `111B7100` can replace the field in a special actor interaction; it is
not a general countdown helper.
This establishes the **行为计时基数** label, without assigning a particular
sleep, recovery or attack action to every consumer.

The root-4 targets expose the scalar accesses proved by `10856B80`, `10841650`,
`108572C0`, `101BC490` and `10844BC0`: signed words at +0/+14/+24/+34/+44/+54/+60
initialize actor fields; +40/+42/+62 are additional signed words, +68 is a
byte limit and +76 is a flags DWORD. At +24, actor+2676 is the **眩晕初始阈值**:
the stun-input path `108BED38` calls `10840B20`, which checks that threshold;
`10840F30` accumulates the input, and `108575B0` raises the threshold after a
completed stun using profile +32. The existing field is signed; zero and
negative thresholds disable accumulation in the gate.

The remaining initial fields need not all stay untyped in meaning, but their
status names are not established solely by their offsets. `10840430` compares
actor+2186 with +2184 and activates bit 0x04 of +2195; `10840740` compares
+2182 with +2176 and activates bit 0x08; `10840FD0` compares +2156 with +2154
and activates bit 0x02; `108413C0` compares +2170 with +2168 and activates bit
0x01. `1004AEF0` compares +3402 with +3400 and activates bit 0x20. These are
established accumulation thresholds; this evidence does not, by itself, name
each status as poison, paralysis or sleep. `101BC710` establishes that +3414
gates a further signed accumulation at +3420, without proving its complete
threshold/trigger path here. `108572C0` consumes profile +42 as an increment
to the +2154 threshold, capped at 30000, and +68 as the maximum number of
such increments. The five established thresholds display as **状态初始阈值**
with their profile-relative offsets. The +42 increment and +68 count limit
explicitly name +0x22 as their target threshold; they do not modify all states.
Other roles retain neutral names. Unlisted bytes remain raw.

For root-10 targets, `1086FB50` / `1087D010` consume nine u16 part thresholds
at +0..+16 and nine u16 increments at +18..+34. `108705C0` dispatches responses
using nine u16 kinds at +72..+88. The threshold is the base plus
`min(increment * 2^(count - 1), 9999)` when actor+3370+part is nonzero, or the
base alone when it is zero (`1087D010` at `1087D02B..1087D05E`). `1086FB50`
compares this result with actor+3314+2*part. Thus the labels are **部位反应阈值**,
**阈值递增基数** and **反应类型**, without equating these counters with root-1
part durability or treating the increment as a fixed additive amount.
The middle +36..+71 span is not assigned a schema; the 90-byte stride does not
imply that every field is understood.

### Species record links

Relocated links are +8, twelve DWORDs at +12, twelve at +72, +176 and +184.
The +8 link selects 24-byte scaling records using actor+3212. The +72 directory
selects anger parameters via `10AA1520`, not a declared list of variant types.
Twelve F32 values at +120..+164 are profile-selected health-threshold ratios:
`1085B5A0` / `1085B620` multiply them by actor+1724. They are not named as capture
thresholds. The +168 word array supplies base values in health calculation;
+188 initializes actor+3392. Other bytes remain raw unless a scalar access is
established.

`111A5080` reads +176 using `40 * (parameter_id + 200 * bank)`, with IDs below
200. `Emd::species_table(species, SpeciesTable::ParameterBank(bank))` exposes
bank 0 and bank 1 as independently checked windows of 200 forty-byte records,
at displacements 0 and 8000. `10050530` also establishes the bank-1 window.
Other banks are rejected because the total bank count is not established;
these views do not claim the entire target allocation. Confirmed scalar fields
are F32 values at +16/+20/+24/+28 and an i16 at +32 (`100606A0` / `10061B10`);
their names remain generic and the other bytes remain raw. The original link
is validated before adding a bank displacement.

`SpeciesTable::AngerProfile(profile)` follows one of twelve nullable +72 links
and exposes only its confirmed 60-byte prefix:

| Offset | Type | Established use |
| --- | --- | --- |
| +0 / +2 | i16 / i16 | Anger threshold / duration, copied to actor+2720 / actor+2724 |
| +4 | F32 | Anger action-speed multiplier, written to actor+2848 |
| +8 | F32 | Anger attack multiplier, multiplying actor+2200 |
| +12 | F32 | Anger incoming-damage multiplier, multiplying actor+2204 |
| +16..+56 | 11 × F32 | Anger gain multipliers selected by health bucket 0..10 |

The threshold is read by `1086F270` / `10856B20`; the duration is read by
`108517E0` / `10851D70` and decremented as signed by `108728A0`. `10851D70` at
`108520B1` / `108520CA` applies the two multipliers. `10851240` at `108515AA`
uses the bucket returned by `108510E0` to select the last array, excluding -1
before using buckets 0..10. It multiplies anger gain, adds it to actor+2722 and
clamps against actor+2720. The view does not infer data after byte 59 or impose
a total allocation size. Zero +72 or +176 links return no view; shared offsets
remain shared, and each nonzero window is checked independently.

The three multiplier display names are **怒态动作速度倍率**, **怒态攻击倍率**
and **怒态承伤倍率**. The additional consumer evidence is:

- Actor+2848 feeds animation-frame tests in `108FF650` / `108FF700` and
  turning increments in `1084D3C0` / `1084D600`. The common actor path
  `1090E5C0` at `1090E7D3..1090E7FD` multiplies it by the base step 2.0 and
  writes all four animation increments at +480/+560/+640/+720. This is an
  action-speed factor, not a distance or an anger accumulation rate.
- `108C1150` at `108C119F..108C121D` reads the attacker's +2200, multiplies
  attack-instance +108 power by it, and accumulates the resulting damage on
  the recipient; its type-8 branch is separate. `108C26B0` at `108C2ADA` /
  `108C2B27` also obtains the multiplier from the damage source.
- `10841DB0` multiplies pending received damage by +2204 before summing the
  result for health processing. `10842A20` applies the same multiplier while
  deducting part durability; `108468E0` at `10846902` also uses it for the
  part-damage clamp. **承伤倍率** preserves this direction: a larger value
  multiplies incoming damage rather than increasing a defense statistic.

These labels are presentation metadata. The existing internal field names,
scalar types and byte bindings are unchanged; native evidence and unresolved
interpretations stay in this document rather than appearing as UI notices.

At +184, the loader processes 200 eight-byte entries,
relocating the first DWORD only when both DWORDs are nonzero; the second DWORD's
full meaning and nested target layout remain unresolved.

`Emd::directory_table(3, species_id)` exposes this fixed-size +184 directory
(or `None` for a zero link). The workbench shows it beneath each species and
lazily expands its 200 records as `target_offset: u32` and `value_04: u32`.
It preserves shared directory offsets and does not interpret `value_04` as a
count, validate unknown target extents, or follow the nested links. Invalid
directory extents are reported independently of the species scalar fields.

### AI script directories and binding

EMD root 9 is an offset directory, relocated by `10AFA180` at
`10AFA3A0` / `10AFA3B4`. Its numbering is independent of the DLL AI descriptor's
`root[1]`, `root[9]` and `root[15 + group]` call tables. An EMD script can be
installed into one of those tables and call other scripts that the EMD payload
does not identify through a complete descriptor.

For Zinogre (species 146), vtable entry +2740 (`10E61520`) chooses an EMD
root-9 entry from a u8 parameter `p`:

| Native branch | EMD root-9 index |
| --- | --- |
| `1087CDE0(actor.species) == 0` | `185 + p` |
| Other ordinary branch | `188 + p` |
| Special species-146 branch: `101CE630() == 0` and `1087CB30(146, 11) == 1` | `p == 0` returns without writing; otherwise `272 + (p - 1)` |

Observed call sites pass 0, 1 or 2; this is not a general bound on the u8 input.
The selected pointer is read at `10E615A9..10E615B2`, then passed to vtable entry
+2684 (`111A88A0`) with slot 1. Its write at `111A88B6` replaces DLL
`descriptor[1][1]`; actor+3270 retains the adjusted parameter (the special
branch uses `p - 1`). Calls into DLL table 18 from the selected script are
downstream references, not its direct binding destination.

Root 17 provides a second script lookup. `10AFA530` matches its species,
actor+3389 and selector keys and returns the +4 pointer. `10E00920` with
selector 3 installs that pointer into DLL `root[1][3]`.

The workbench displays root-9 and root-17 targets as bounded instruction lists,
reachable from species associations or the unassigned global records, with
opcode, operand bytes and the original instruction bytes. Known root-9
associations show candidates and their selection parameters; they do not assert
which candidate is active in a running game. The workbench provides no DSL
text view or complete project export. It reuses
`mhf_monster::ai::decompile::extract_script` and the shared bytecode codec:
native conditional markers must close before a return or other proven terminal
ends the view; the reader is bounded by the payload and its 64 KiB search
budget. It does not size scripts using the next offset. Since a directory entry
alone does not establish a call level, `81/82` are not assumed to be same-level
tail calls; explicit `FF 01/02` returns are recognized.

Calls retain their DLL table/index references. This is an instruction preview,
not a complete DSL project or proof that every referenced script is present.
An unreadable or structurally unsupported entry reports its own diagnostic;
the pointer remains editable and a bounded raw preview is available for an
in-range target. Other entries remain inspectable. Fixed-size edits change the
original decoded buffer and use the normal envelope repacking path.

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

Key native consumers also include `10846950` / `108474F0` (root 1),
`10847410` / `10856B80` (root 2), `10859100` (root 7/8 and species scaling/health),
`1085F680` and adjacent helpers (root 13), `1085F920` (root 14), `1006E310`
(root 15/16), `10AFA590` (root 18), and `10851990` / `108474F0` (root 22).

This is **not a complete semantic decode**. Unresolved roots, nested target
lengths, unknown fields inside fixed-stride records, and bank counts still need
consumer tracing and real-asset verification. In particular, unused-looking
roots are not declared empty merely because no consumer has been found.
The 21 known root views leave roots 8/20/23 unresolved. Species +8 and +12
links still lack proved complete target extents; +184 target semantics remain
unconfirmed even though its 200-entry directory is exposed. The two +176 bank
windows and each +72 prefix do not establish bounds for other banks or tails.

The workbench exposes known tables and directory targets lazily. Confirmed
scalar accesses are editable; other spans remain raw. Data stays in the original
backing buffer and fixed-size edits use normal envelope repacking. No variant
support flags are inferred, and variant behavior is unchanged.

Enumeration uses the header count, including zero/placeholder slots. A slot is
not proof of a spawnable monster or an available AI descriptor. Chinese labels
in `species.rs` are display metadata only; unnamed IDs use `emNNN`. Counts above
the known name range remain visible.

## Other species-ID constraints

- Debug catalog: candidates come from the loaded, relocated EMD count. The
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

The two resource samples are identified by their encoded SHA-256 below.
Sample A accompanies the analyzed DLL with SHA-256
`95c580195f4080d2e9582c8c9df36abeb280476e088b6366583c5f138da8f301`.
Sample B accompanies a different DLL; resource layouts do not establish
executable compatibility.

| Sample | Encoded SHA-256 | Decoded bytes | Species slots | Root 22 |
| --- | --- | --- | --- | --- |
| A | `fc1cc983f1b3411fb62f256eb40f31984da3a160747d37cd8cf80a26d57d8c81` | 2,572,800 | 177 | offset 2,571,008; 64 records |
| B | `e68e03f1274d9d055cc8e42b0427bb99b0cebfe9c905f951900b991f4cc8afd1` | 2,571,008 | 177 | offset 35,968; 40 records |

Both samples pass all 21 known root views and the supported nested-directory
checks, including 184 nonnull parameter-bank windows and 1,464 nonnull anger
profile prefixes. Workbench scalar-edit/envelope-repack/reopen checks compare
the entire decoded payload against the original with only the requested edit.
Tests read local resources without modifying the installed files.

Both samples have 505 root-9 entries: the shared codec extracts 470 and reports
boundary or structure diagnostics for 35. For example, entry 274 has an
unmatched `80/02` marker. Unsupported selectors or missing execution context
remain per-entry diagnostics with raw previews; these are not claims that the
original native scripts are invalid. The eight observed Zinogre selections,
185..190 and 272..273, pass extraction. All 85 root-17 entries in each sample
pass the same checks and end in `FF 01`. These counts describe the checked
resources, not hard-coded parser limits or game execution validation.

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
stride among 4, 8, 16 and 32 that fits the whole sample A: 3,755 non-null
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

Nonzero `value_04` values range from 1 to 74, and no shared target carries
conflicting values. Under the candidate list interpretation, +0 is
nondecreasing in only 814 of 3,755 lists, and 155 records carry an odd +0 value,
so +0 is not established as a frame or time. The two 0/1 lanes and the six-value
lane look enumeration-like,
but no field here has been tied to a named native consumer, and the parser and
workbench keep the opaque link fields instead of enforcing this layout.

### Shared index space with +176 (evidence, not naming)

`sub_10050530` rejects an index of 200 or more, then reads
`parameters_176[40 * index + 8000]` and returns the DWORD at record +16. The
bank-1 view therefore covers 200 forty-byte records at displacement 8000;
bank 0 is separately established by other consumers. Each bank's validated
index range is 0..199, the same cardinality as the +184 directory. These two
windows do not establish the total allocation or exclude additional banks.

Measured on the sample: over the 126 species carrying data in either table, the
populated +184 indices are a strict subset of the populated +176 indices, and
the only index populated in +176 but never in +184 is 0. This links both tables
to one index space without establishing what selects that index.

### Static consumer audit

For the DLL accompanying sample A, the current IDA database reports 2,185 direct
references to the decoded EMD global at `1E77DCE0`, in 1,455 functions. Full
disassembly and pseudocode cover those functions. Direct-reference coverage
does not exhaust possible aliases or indirect call paths.

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

Unresolved root offsets in both samples are 8=160, 20=37,472 and 23=35,936.
Root 8 starts with floats 0.3, 0.3, 0.35, 0.4, but these values do not establish
an array length. Root 12 at 67,872 has the separately established `N * 36`
species/part-ratio view; its leading 0.01 values do not establish a larger
allocation. The different root-22 placement must not be used to infer root-23
size.

Reproduce with `MHF_EMD_PATH=/path/to/dat/mhfemd.bin`:

```sh
cargo test -p mhf-resource --target aarch64-apple-darwin --test emd_real -- --ignored --nocapture
cargo test -p mhf-workbench --target aarch64-apple-darwin real_emd_ -- --ignored --nocapture
```

The AI-directory check accepts encoded resources or decoded payloads.

Synthetic boundary tests remain separate. Complete nested semantics and
in-game UI validation are still outstanding.
