# AI debugger core

`mhf-ai-debug` provides portable dispatch breakpoints, scalar field watchpoints,
bounded instruction traces, JSON recordings and offline captured-state replay.
It has no Windows or UI dependency. The game adapter supplies the actual
instruction transitions, identifies monster generations and normalizes cursors
to program revision, script ID and byte offset.

`Debugger::before_instruction` decides whether the next dispatch may run.
`after_instruction` records its state changes and handles watchpoints, single
step and bounded run-until-yield. Field watchpoints also compare the next
captured input with the last completed instruction, so external lane or state
changes can pause before the next body executes. These identify the next
observed dispatch, not the exact external write. Before the first captured
instruction there is no baseline from which to detect an earlier change.
Continuing from a breakpoint before dispatch bypasses that same dispatch once;
a watchpoint after an instruction does not suppress the next instruction's
breakpoints. Field conditions read captured values only.
Pausing is cooperative; the backend owns continuation registers, the native
dispatch budget and interaction with the game scheduler.

Recordings distinguish writes during a dispatch from input changes between
dispatches. A full trace buffer evicts its oldest entry and advances the initial
snapshot, retaining a replayable suffix and an explicit dropped-prefix count.
Import rejects missing sequence numbers, contradictory deltas, different monster
instances, backwards frames and inconsistent checkpoints. Captured script images
are checked against recorded instruction bytes when present. Imported data never
contains an instruction to read or write process memory.

Replay validates and reconstructs **recorded fields**. It does not execute native
callbacks, prove that all engine side effects were captured, or predict a changed
script. The format marks these recordings as `Observation`. Seeking and stepping backwards
restore a verified checkpoint and replay forward.

The in-game debugger UI uses `ReplaySession` to inspect recordings without
re-executing their instructions. Host tests can override the workspace's default
Windows target:

```sh
cargo test -p mhf-ai-debug --target aarch64-apple-darwin --offline
```
