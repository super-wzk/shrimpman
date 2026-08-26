# Shrimpman

## Documentation

- [Domain language](docs/domain-language.md)

## Build prerequisites

The Protocol Buffers compiler (`protoc`) must be available on `PATH` before
building the workspace.

The development container also includes [`just`](https://just.systems). Run
`just` to list the available tasks. Apply migrations once before starting the
services:

```sh
just db migration apply
```

Run each service in a separate terminal:

```sh
just entrance
```

```sh
just sign
```

```sh
just world
```
