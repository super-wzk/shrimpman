# Native text behavior

The supported unpacked image uses preferred base `10000000`. Addresses here
are virtual addresses at that base; hook definitions store RVAs.

| Behavior | Entry | Adapter |
| --- | --- | --- |
| Markup display columns | `10B72180` | `layout::measure` |
| Markup pixel width | `10B72680` | `layout::measure` |
| Center within an available width | `115A1630` | shared centering dispatch |
| Maximum line columns | `10B723B0` | `layout::measure` |
| Variable-string width | `10886110` | shared variable expansion |
| CRT `_strlen` | `115BB570` | unchanged byte-count contract |

CRT `_strlen` retains its byte-count contract for allocation, copying and
formatting. Display adapters handle character measurement at the relevant
consumers, including compiler-inlined loops that do not call `_strlen`.

For example, title-menu code at `1083CA72..1083CA7D` increments a byte pointer
until NUL. `1083CA85` subtracts the start pointer and `1083CA8F` multiplies by
the font-width quarter to obtain half the displayed label width. This is an
inlined display measurement, so its narrow adapter substitutes Unicode display
columns while preserving the native continuation and coordinate arithmetic.
It uses the same `utf8::display_columns` primitive as the common width paths.
The dedicated adapter is necessary because there is no shared callee there.

Keep byte counts for buffer capacities, protocol fields, filenames and cursors.
Convert known legacy source encodings at resource/native/OS boundaries; measure
display columns only at verified layout consumers. Native data structures and
their translation keys are documented in `../resources/native/README.md`.
