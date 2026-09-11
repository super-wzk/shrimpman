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
| 0 | u16 | Retained, meaning unconfirmed; all audited samples contain 1 |
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
All 2,130 kind 1 members in the initial ordinary/HD package audit have version 4,
zero counts at offsets 14/16/18, and no bytes after the six fixed tables.

`113D3FC0` matches emission selector +80, checks trigger frame +78, and emits
`i16 +88` instances. The definition ID at +76 matches the u16 at +4 in the
selected 56/140-byte table. ID `0xffff` is also used to identify looping
emissions (`113D4750`). Fields +82 and +92 are passed to spawn helpers but their
complete semantic names remain unconfirmed.

Emission offsets 0/12, 24/36 and 48/60 hold position, rotation and scale vectors
and their random ranges. `113D37E0`, `113D3BD0` and `113D3D80` respectively apply
those pairs; all floats are represented as original u32 bits. The remaining
emission fields and definition bytes stay raw rather than receiving guessed
names.

`113CCD10/113CCD90` establish 24-byte vector keys: three float bits at +0,
signed frame at +12, flags at +16, curve ID byte at +18. `113CCAB0/113CCAF0`
establish 16-byte color keys: u32 frame +0, flags +4, curve ID byte +6, RGBA +8.
`113CC8A0/113CC970` establish 16-byte integer keys: u32 frame +0, flags +4,
u16 curve ID +6 and i32 value +8. These parsers preserve order, duplicate
keys, unknown bytes and original bits; they do not resample curves.

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

Signed ranges matter: em145 has -25112..1128 with 26,240 slots, and em142 has
63,537 slots. Large tables mostly contain `0xffffffff`; no slots are collapsed.
The eight zero bytes in em150 are a valid empty event resource.

## Sample verification

Initial full scan: 502 packages with nonempty member 5, containing 2,130 banks
and 482 motion-event members. The regular and HD em001 packages both contain
bank IDs 149 and 153 (8,768 and 5,020 bytes). em004 contains bank ID 152;
ordinary/HD sizes differ (11,648 and 13,588 bytes), so HD payloads cannot be
assumed identical to ordinary payloads.

Synthetic tests cover descriptor mismatches, array truncation, aliases, unknown
kinds, empty resources, signed motion ranges, sentinel and invalid indices, and
byte-identical record serialization. The optional client audit uses
`MHF_CLIENT_DATA_DIR` and the decoded motion-event audit uses
`MHF_EFFECT_SAMPLE_DIR`; neither requires committed proprietary fixtures.
