# Shrimpman

## Documentation

- [Domain language](docs/domain-language.md)

## Development

The root [flake environment](../README.md) provides Rust, the Protocol Buffers
compiler and the other tools needed by both workspaces. This workspace's
[`default.nix`](default.nix) defines server commands and the process graph.

From the repository root:

```sh
nix develop --impure
shrimpman-dev up
```

Sign and World wait for a successful database migration, and all services wait
for etcd to become healthy. Selected services include their dependencies:

```sh
shrimpman-dev up shrimpman-entrance
shrimpman-dev up shrimpman-sign
shrimpman-dev up shrimpman-world
shrimpman-build
shrimpman-migrate
shrimpman-db --help
```

The same commands work directly through Nix, without entering the shell:

```sh
nix run .#dev -- up
nix run .#shrimpman-db -- migration status
```

See the root README for the `local` module, environment overrides and data paths.
