# Debug task preset

`test-map.bin` is the debug module's default task: the original Japanese quest 55921
(Blinking Nargacuga), expanded from its JKR container without translating its text.
The preset starts all four hunters in Historical Site camp 460 and clears the
random-spawn flag. These choices belong to Debug; Quest receives the resulting
BIN as ordinary caller-supplied bytes and owns only validation and lifecycle.

Set `quest` in `[mods."mhf.debug".settings]` to select a different BIN/JKR file.
An empty or invalid input is an error; Quest has no implicit or built-in task.
