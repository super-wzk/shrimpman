# Native text sources

The unpacked client contains both typed data and compiled string constants.
`layout.rs` is shared by the build-time dictionary validator and runtime image
reader; it defines structure, not translated text.

- `native:rank:0..4`: five 8-byte records at RVA `01923254` with a text pointer
  at offset 0 and rank number at offset 4. The indexed readers at VA `10445D06`
  and `104468AE` use the label; `10445CFB` and `10446872` use the rank number.
- `native:room:0..5`: six consecutive pointer fields at RVA `019DDB0C` for
  vacant, configuring, unknown, public tavern, social tavern and competition
  venue. `1155C740` dispatches by room type 210/282/286; other state branches
  consume the first two fields. This is a block of named fields, not an
  inferred open-ended string table. The following debug-name directory is excluded.
- `native:literal:<decimal source RVA>`: 109 scattered constants whose actual
  instruction operands or data aliases are recorded in `bindings.rs`.
  Bindings contain only the source RVA and consumer pointer RVAs. Text is read
  directly from the loaded image; no source-byte copy or opcode fingerprint is
  stored. This does not discover text by scanning the image or guessing encodings.

All 120 records are in `translations/ja-JP.jsonl`. Other locales can use the
same keys and the existing `translation` and missing-translation options.
With no Translation configuration, the known CP932 source ingress is still
converted to UTF-8. The native image keeps its original bytes; only the
consumer pointers are changed. All pointers are prepared before writes begin,
and rollback restores them before freeing the corresponding arena or DLL.

`119A04B0` contains seven fullwidth spaces. Ten byte-copy operands start at
index zero and use 128-byte fields; their displacements and alias `1187CE20`
remain explicit bindings. The four other indexed padding pools keep their
native addresses and are handled by the common strcat source-range adapter.

## Deliberate exclusions

| Source / consumer | Reason |
| --- | --- |
| `1197A068` → `1158EE47` → `CreateFontA` | OS font family (`ＭＳ ゴシック`). |
| `1199F600`, `1199F744`, `1199F908` | OS controller names used by `_mbscmp` at `114D0FF4`; not game display text. |
| `119A0138`, memory/DirectX/OS/fatal error strings | `MessageBoxA` captions and message bodies. |
| `119DE034..119DE060`, `119B1A14/1A40/1A74` | Connection messages route through `1150B3E0` → `MessageBoxA` at `1150B40D`. |
| `119B2FA4..119B30E0` enumerated debug strings | `1158C220` → `OutputDebugStringA` at `1158C256`. |
| Death-match/AQ/matching/area-route debug strings | `nullsub_2` at `10164CA0` is a bare `RET`. |
| `119B430C`, `119B4324` | Screenshot path construction has a separate Unicode OS-boundary implementation. |
| `1199F260` / header vector at `11A45268` | No code consumer of this duplicate static XML template was established. The active writer at `108D75E0` uses PAC `table_793:0`; its UTF-8 declaration is fixed at that exact resource ingress. |
| `11997F6C`, `119A6D50`, word loads from `119A02C4`, `119B2DB8` | Fixed-size constructors and digit indexing are replaced in `native/embedded.rs`. The `10B2B2A5` pointer consumer of `119A02C4` is included in the bindings. |

The following 33 semantic strings have typed data-pointer references but no
established executable reader in the current IDB. They are archived as
unreferenced data, excluded from the bindings, and are not counted as migrated:

- Monster states: `1198156C`, `11981574`.
- Rock-paper-scissors: `119961DC`, `119961E4`, `119961EC`.
- Owner brackets: `11996554`, `11996560`.
- Support states: `1199EAF4`, `1199EB00`, `1199EB04`, `1199EB0C`.
- Colors: `1199FA6C`, `1199FA78`, `1199FA84`, `1199FA90`, `1199FA9C`,
  `1199FAA8`, `1199FAB8`, `1199FAC4`, `1199FAD0`, `1199FADC`, `1199FAE8`,
  `1199FAF4`.
- Body parts: `1199FB00`, `1199FB08`, `1199FB10`, `1199FB18`, `1199FB20`.
- Support ammunition: `119A037C`, `119A038C`, `119A039C`, `119A03A8`, `119A03B4`.
