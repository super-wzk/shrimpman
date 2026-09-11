# Grouped material parameters

These bytes are a separate monster-package resource consumed by `108FCD70` in
the local `mhfo-hd.dll`. They are not an FMOD section and have no magic signature.

## Caller contract

`108FBA70` loads an HD monster package, with an ordinary-package fallback. At
`108FBEA4..108FBED7`, it checks the outer PAC count is greater than 6 and member
6 has nonzero size. It sets ECX to `PAC base + DWORD[13]` (member 6's offset),
EAX to a 16-byte output header, and calls `108FCD70` with two stack output
pointers for group headers and expanded records.

After parsing, the caller compares the signed byte at output header +0 with
the primary loaded model's group count at runtime object +0x2C. It then compares
each group's first signed byte with the corresponding runtime material count.
Matching records are copied to existing 140-byte runtime material structures.
These association checks do not change how the file itself is parsed.

The additional-monster path `108FC420` consumes outer members 0, 1, and 2 only;
it does not call `108FCD70`. The latter function has one caller in this database.
Member 5 is separate: `108FBE88..108FBE99` dispatches it to `113D5A90`, which
decompresses an indexed archive. Its first member holds u16 type/ID pairs;
types 1 and 2 dispatch subsequent members to `113D5620` and `113D5380` with
their payload, byte length, and ID. It is not the grouped-material format.

For automatic inspection, identify member 6 only in an outer package with at
least seven members whose member 0 has already been established as the primary
nested geometry/skeleton resource and whose member 1 is its texture bundle.
The first byte 0x20 by itself is insufficient. A contextual parser may retain
extra bytes; neither 0xCC padding nor exact file exhaustion is a universal magic
or structural requirement.

## File layout

`108FCD70` tests the unsigned first byte. If it is at least 0x20, that byte is
skipped and records are 100 bytes. Otherwise it is part of the file header and
records are 96 bytes. `GroupedMaterials::version_marker` retains the exact
optional byte; it is not replaced with a normalized version enum.

The file header and every group header are each 16 bytes. Byte +0 is a signed
count; bytes +1..+15 are retained without interpretation. The file header counts
groups. Each group header is immediately followed by its counted records.
Negative counts are rejected before allocation, and every header and complete
record must fit the input. Empty groups retain their ordinals.

| Record range | Representation |
| --- | --- |
| 0x00..0x10 | `color_00: [u32; 4]`, original float bits |
| 0x10..0x20 | `color_10: [u32; 4]`, original float bits |
| 0x20..0x30 | `color_20: [u32; 4]`, original float bits |
| 0x30..0x48 (96-byte record) | Six original float parameter words |
| 0x30..0x4C (100-byte record) | Seven original float parameter words |
| Final 24 bytes | Original unknown bytes |

The outer caller copies the three four-float vectors into runtime material
offsets +0x04, +0x24, and +0x58. It also copies the parameters at normalized
record +0x30..+0x48 to material fields, but their shader meanings are not named.
No gamma, color-range, NaN, or signed-zero conversion occurs in the parser.

For legacy records, native `108FCD70` copies 96 bytes into a 100-byte allocation,
shifts source words +0x3C/+0x40/+0x44 to runtime +0x40/+0x44/+0x48, and writes
zero at runtime +0x3C. The file parser deliberately does not perform this runtime
conversion. `parameter_words` therefore has six elements for legacy records
and seven for extended records. `as_bytes()` returns the unchanged source;
file tails, header unknown bytes, and all unknown record bytes remain intact.

## Verified local samples

| Outer resource | Entry 6 offset in decoded PAC | Bytes | Groups | Records |
| --- | --- | ---: | ---: | ---: |
| `emmodel-hd/em019-hd.pac` | 0xBD996 | 833 | 1 | 8 |
| `emmodel-hd/em077_b-hd.pac` | 0x55DE79 | 4277 | 10 | 41 |

Both samples begin with marker 0x20. In em019-hd the next 16 bytes are
`01 CC CC CC CC CC CC CC CC CC CC CC CC CC CC CC`, followed by the group header
`08 CC CC CC CC CC CC CC CC CC CC CC CC CC CC CC`. Records start at 0x21 and
consume exactly `1 + 16 + 16 + 8 * 100 = 833` bytes. em077_b-hd consumes exactly
`1 + 16 + 10 * 16 + 41 * 100 = 4277` bytes. Its group record counts are
`[1, 4, 1, 9, 11, 2, 7, 2, 3, 1]`.

Tests cover both native record widths, arbitrary marker bytes in the native
extended range, empty groups, raw float bits, unknown header/record bytes,
trailing bytes, all declared-header/record truncations, and invalid counts.
`MHF_RESOURCE_MATERIAL_SAMPLE` enables an additional local decoded-member test.
