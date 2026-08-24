# Shrimpman

## Build prerequisites

The Protocol Buffers compiler (`protoc`) must be available on `PATH` before
building the workspace.

The development container also includes [`just`](https://just.systems). Run
`just` to list the available tasks, or start the Sign service directly:

```sh
just db migration apply
just sign
```
