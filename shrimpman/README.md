# Shrimpman

## Documentation

- [Domain language](docs/domain-language.md)

## Text encoding

Sign and Entrance exchange text as UTF-8. Sign rejects invalid UTF-8 credentials;
outbound strings preserve the domain text without replacement or truncation.
Use the matching UTF-8 client: legacy code-page conversion is no longer part of
these services.

The binary layouts and byte limits remain unchanged:

| Field | UTF-8 payload limit |
| --- | --- |
| Sign character name | 15 bytes, followed by NUL in a 16-byte field |
| Sign character description | 31 bytes, followed by NUL in a 32-byte field |
| Sign login notice | 65,534 bytes; its u16 length includes the trailing NUL |
| Sign server address or relation name | 254 bytes; its u8 length includes the trailing NUL |
| Entrance world name and description | 63 bytes combined, plus two NUL bytes in a 65-byte field |

These are byte limits, so multibyte characters consume more than one byte.
HTTP JSON, configuration, database text and password hashing already use Unicode
strings and need no data migration. Existing `savedata` blobs remain opaque and
are not transcoded; World currently handles login and ping, not chat or save-data
text. The matching launcher converts legacy resource text at load time. Existing
legacy savedata still requires a format-aware migration when its text is exposed;
opaque binary blobs must not be decoded or rewritten as whole UTF-8 strings.

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
