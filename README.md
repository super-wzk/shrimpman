# Shrimpman

The repository contains two Rust workspaces:

- `shrimpman/`: Sign, Entrance, World and persistence. See [domain language](shrimpman/docs/domain-language.md).
- `mhf/`: the Windows launcher and Direct3D 9 overlay. See [launcher documentation](mhf/crates/mhf-launcher/README.md).

Install Rust, Protobuf and Just. Run `just db migration apply` before starting services with `just sign`, `just entrance` and `just world`.

Build the Windows launcher with `just mhf-build`; `just mhf-launch` uses the game directory from `.env`.
