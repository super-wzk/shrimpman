# Packaged effect banks and motion events

Addresses refer to the supported ZZ HD `mhfo-hd.dll` at its preferred image
base. Resource bytes retain offsets and encoded fields; they are not native
pointers. No client files are included in this crate.

## Package contract

`108FBA70` checks outer archive member 5 at `108FBE88..108FBE99` and passes its
JKR data to `113D5A90`. Ordinary and HD model packages share this consumer.
After decompression, the member is an ordinary offset/size archive with a
descriptor in inner member 0:

| Offset in member 0 | Storage | Native use |
|---|---|---|
| 0 | u16 | Retained, meaning unconfirmed; parsing does not require a fixed value |
| 2 | u16 | Descriptor count; must equal inner archive count minus 1 |
| 4 | `(u16 kind, u16 resource_id)` repeated count times | One descriptor per subsequent member |

Kind 1 calls `113D5620 -> 113D4A70`; kind 2 calls `113D5380 -> 113D4BB0`.
Other kinds are skipped by the native dispatcher and preserved as unknown
members. The descriptor resource ID selects the loaded effect bank/event table;
it is separate from the per-bank emission and definition IDs.

`EffectArchive::parse` validates the archive and descriptors and retains all
original records, IDs, offsets, sizes and bytes. `EffectMember::resource` parses
one typed payload on demand, so aliased members do not cause repeated eager
allocation of entire banks. Unknown descriptor and payload tails survive.

## Kind 1: effect bank

`113D4A70` requires the leading u16 to be at least 4, starts at byte 28, and
advances through the following tables in order. Header bytes 20..28 are retained
without assigning a meaning.

| Count offset | Record size | Exposed data |
|---|---:|---|
| 2 | 112 | Emission records |
| 4 | 24 | Three-component curve keys |
| 6 | 16 | RGBA curve keys |
| 8 | 16 | Discrete integer curve keys |
| 10 | 56 | Effect definitions selected when emission flags bit 0 is clear |
| 12 | 140 | Effect definitions selected when emission flags bit 0 is set |
| 14 | 4-byte range, then count × 4 | Optional motion lookup |
| 16 | 32 | Optional motion events |
| 18 | Unconfirmed | Address retained by loader; remaining bytes kept raw |

The optional motion lookup and event records have the same layouts as kind 2.
Zero counts omit those tables. The parser uses the declared counts rather than
assuming version 4, empty optional tables, or exhaustion after the six fixed
tables. The final unconfirmed table and any remaining bytes are retained raw.

`113D3FC0` matches emission selector +80, checks trigger frame +78, and emits
`i16 +88` instances. The definition ID at +76 matches the u16 at +4 in the
selected 56/140-byte table. ID `0xffff` is also used to identify looping
emissions (`113D4750`). Fields +82 and +92 are passed to spawn helpers but their
complete semantic names remain unconfirmed.

Emission offsets 0/12, 24/36 and 48/60 hold position, rotation and scale vectors
and their random ranges. `113D37E0`, `113D3BD0` and `113D3D80` respectively apply
those pairs; all floats are represented as original u32 bits. The remaining
emission fields and unconfirmed definition regions retain their original bytes.

`113CCD10/113CCD90` establish 24-byte vector keys: three float bits at +0,
signed frame at +12, flags at +16, curve ID byte at +18. `113CCAB0/113CCAF0`
establish 16-byte color keys: u32 frame +0, flags +4, curve ID byte +6, RGBA +8.
`113CC8A0/113CC970` establish 16-byte integer keys: u32 frame +0, flags +4,
u16 curve ID +6 and i32 value +8. These parsers preserve order, duplicate
keys, unknown bytes and original bits; they do not resample curves.

### 56-byte definitions and curve references

`113CD4B0` constructs the runtime instance from a 56-byte definition. The
following names describe fields supported by its reads and their consumers;
unconfirmed regions remain available as raw bytes.

| Definition offset | Storage | Exposed field / native use |
|---|---|---|
| 0x00 | u32 | `flags`, retained as individual bits without speculative labels |
| 0x04 | u16 | `definition_id`, matched by the emission record |
| 0x06, 0x08 | u16 each | Unconfirmed values |
| 0x0a | u16 | Integer-curve selector; semantic role unconfirmed |
| 0x0c | u16 | `duration_steps`, threshold for the native age counter |
| 0x0e | u16 | Position-curve selector |
| 0x10 | u16 | Rotation-curve selector |
| 0x12 | u16 | Scale-curve selector |
| 0x14 | 12 bytes | Unconfirmed region |
| 0x20 | u16 | Color-curve selector |
| 0x22 | u16 | Vector-curve selector; semantic role unconfirmed |
| 0x24 | i16 | Integer-curve selector, sign-extended by the constructor |
| 0x26 | u16 | Vector-curve selector; semantic role unconfirmed |
| 0x28 | 16 bytes | Unconfirmed region |

The constructor resolves vector selectors through `113CCD10`, color through
`113CCAB0`, and integer through `113CC8A0`. The position, rotation and scale
references occupy runtime offsets +0xb4, +0xc0 and +0xcc and are consumed by
`113CECF0`, `113CF6B0` and `113CFFA0`. These are references into bank-wide key
tables: a definition does not own or duplicate those physical records.

Vector and color key IDs occupy one byte, but lookup compares that byte with
the complete definition selector. For example, selector `0x0101` does not match
key ID `1`. Integer key IDs are unsigned words; definition +0x24 is instead
sign-extended, so its stored `0xffff` passes `-1` and does not match key `65535`.
Zero is an ordinary possible key ID, not an implicit absent-curve marker.

Each lookup helper records the first matching key and the total number of
matches. Evaluators traverse that contiguous first-plus-count span without
checking every subsequent key's ID again. Matching records need not be adjacent:
for IDs `[7, 8, 7]`, selecting `7` finds indices `[0, 2]`, while the native span
is `[0, 2)` and includes key `8`. `curve_lookup` exposes both the original
matching indices and this native span. It preserves order and does not sort,
regroup or reject interleaved keys. Workbench presents both views and binds its
read-only reference to the actual key-table span; editable key records retain
their physical table as their sole parent.

`113CDD70` compares the age counter against definition +0x0c after instance
time scaling, handles ending/repeating at the threshold, and advances the age
counter by two. `duration_steps` therefore does not express seconds, nor does
it model owner lifetime, repeat flags or the complete runtime lifecycle. The
140-byte definition remains a raw record apart from its confirmed ID at +4.

## Kind 2: motion events

`113D4BB0` copies the payload and points its runtime tables into that copy. It
does not relocate pointers within the source. The eight-byte header contains
unknown u16 fields at +0/+6, lookup count N at +2 and event count M at +4.
When N is nonzero, byte +8 contains signed i16 `[start,end)` and byte +12 starts
N u32 first-event indices. `0xffffffff` is an empty slot. Events then occupy
M × 32 bytes. With N zero, the event array starts at byte +8.

| Event offset | Storage | Native use |
|---|---|---|
| 0 | 3 × f32 bits | Position delta |
| 12 | i16 | Motion ID |
| 14 | i16 | Trigger frame |
| 16 | i16 | Bone/node index |
| 18 | i16 | Emission selector; bit 15 is handled separately by native spawn |
| 20 | i16 | Kind 1 resource ID |
| 22 | u16 | Original flags |
| 24 | 8 bytes | Unconfirmed tail |

`113D60B0 -> 113D4C50` looks up the entity's motion ID and walks consecutive
events until the ID changes. `MotionEvents::events_for_motion` follows that
ordering and handles absent slots explicitly. It does not imitate the native
`-1` index's read before the event table. Indices remain inspectable even when
invalid; accesses report an error.

Ranges are signed: a negative start is valid and must not be converted to an
unsigned motion ID. Large lookup tables can contain many `0xffffffff` slots;
those slots retain their original indices. An eight-byte zero header is a valid
empty event resource.

## Verification

Ordinary and HD packages use the same format, but equal resource IDs do not imply
identical payloads. For example, bank 152 in `emmodel/em004.pac` and
`emmodel-hd/em004-hd.pac` has different contents and lengths. Consumers and tools
must use the bytes belonging to the selected package.

Synthetic tests cover descriptor mismatches, array truncation, aliases, unknown
kinds, empty resources, signed motion ranges, sentinel and invalid indices,
definition selector widths, interleaved curve keys, and byte-identical record
serialization. Workbench tests also edit definitions and physical key fields
inside a JKR envelope while retaining unknown bytes and unchanged float bits.
Optional integration tests accept a client `dat` directory through
`MHF_CLIENT_DATA_DIR` and decoded motion-event samples
through `MHF_EFFECT_SAMPLE_DIR`. No proprietary fixtures are committed.

```sh
MHF_CLIENT_DATA_DIR="<client-dat-directory>" \
cargo test -p mhf-resource --target "<host-target>" \
  --test effect_archive_native_samples -- --ignored

MHF_CLIENT_DATA_DIR="<client-dat-directory>" \
cargo test -p mhf-resource --target "<host-target>" \
  --test effect_definition_records -- --ignored
```

Run from the `mhf` workspace with `<host-target>` replaced by the host triple
reported by `rustc -vV`. Parsing and byte-preserving tests do not establish
rendering, effect lifetime, or successful in-game use of an edited resource.
