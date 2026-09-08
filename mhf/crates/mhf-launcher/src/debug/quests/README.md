`55921d0.bin` is the original JKR-compressed Blinking Nargacuga quest from
the [quest archive linked by Erupe](https://files.catbox.moe/xf0l7w.7z).
Its native map ID is 97 (Historical Site / 古迹), with camp 460 and arena 461.

The BIN is compiled into `mhf-debug-launcher` with `include_bytes!`; running it without `--quest`
needs no external quest file. `Quest::test_map()` decompresses it, appends Chinese
UTF-8 text for all eight fields (title, main objective, two sub-objectives,
success, failure, contractor and description), and updates their relative
pointers before native relocation. Hunter spawns are fixed to camp 460, retaining
the original camp coordinates for all four players.

The source BIN is unchanged. Area mappings, transitions, monsters and resource
definitions retain their original offsets and contents. Passing an explicit
`--quest <BIN>` loads that file with its own text and spawn configuration.
