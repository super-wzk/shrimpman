# Shrimpman development

The repository contains two Rust workspaces:

- [`shrimpman/`](shrimpman/README.md): Sign, Entrance, World and persistence.
- [`mhf/`](mhf/crates/mhf-launcher/README.md): the 32-bit Windows MHF launcher and overlay.

## Development environment

Install Nix with `nix-command` and `flakes` enabled. From the repository root:

```sh
nix develop --impure
shrimpman-dev up
```

The root `flake.nix` declares inputs and supported systems. Its flake-parts module
`development/flake.nix` assembles the following modules and exposes the runnable apps,
packages and checks:

| File | Responsibility |
| --- | --- |
| `development/shell.nix` | Shared development options, devShell and command helpers |
| `shrimpman/default.nix` | Server tools, commands and process definitions |
| `shrimpman/config.nix` | Server application defaults and TOML generation |
| `mhf/default.nix` | Cross-compilation, launcher and Wine/WSL execution |
| `mhf/config.nix` | MHF application defaults and TOML generation |
| `local/default.nix` | Optional machine-specific overrides |

Only the root is a flake; `flake.lock` pins Nixpkgs, the Rust overlay, flake-parts and
[process-compose-flake](https://github.com/Platonic-Systems/process-compose-flake).
flake-parts manages platform outputs through `perSystem`, with one Rust-overlay
package set per platform. `process-compose.shrimpman-dev` imports the application
and development modules using process-compose-flake's standard flake-parts interface.
`rust-toolchain.toml` defines Rust components and the Windows target.

The environment includes Rust, etcd, Protobuf, Cargo Xwin and LLVM tools. It
supports Apple Silicon macOS and aarch64/x86_64 Linux. On x86_64 Linux it also
provides Wine; on macOS, use your existing Wine installation.

For automatic activation, install `direnv` outside the project shell and add
`eval "$(direnv hook zsh)"` to `~/.zshrc` (use `bash` for Bash). Then run:

```sh
direnv allow
```

[`.envrc`](.envrc) uses a version-pinned nix-direnv, watches the toolchain and Nix
modules, and loads the optional `local/` module with `--impure` when present.
Entering the shell loads tools and environment variables; processes start explicitly.

## Commands

The development `apps` enter the devShell through `nix develop`
before running, so Nix setup hooks initialize the compiler, SDK and libraries.
Commands inside direnv/devShell run directly. Add `--impure` to include the optional
local module; the wrapper uses the same flake snapshot and preserves that module selection.
The development shell exposes the same executables:

`apps` contains runnable entries for `nix run`; `packages` contains buildable
artifacts; `checks` contains checks run by `nix flake check`. The standalone
`update-configs` app only regenerates public TOML files and does not enter a devShell.

| Run from the repository | Command inside the shell | Purpose |
| --- | --- | --- |
| `nix run .#dev -- up` | `shrimpman-dev up` | Start the development processes |
| `nix run .#shrimpman-build` | `shrimpman-build` | Build the server workspace |
| `nix run .#shrimpman-db -- --help` | `shrimpman-db --help` | Run the database CLI |
| `nix run .#shrimpman-migrate` | `shrimpman-migrate` | Apply database migrations |
| `nix run .#mhf-build` | `mhf-build` | Build the Windows launcher |
| `nix run .#mhf-launcher` | `mhf-launcher` | Build and launch for the current host |

```sh
shrimpman-dev up shrimpman-sign     # Sign, etcd and migration
shrimpman-dev up shrimpman-entrance # Entrance and etcd
shrimpman-dev up -t=false           # run without the TUI
shrimpman-dev process list
```

The workspace modules declare processes and dependencies. Services wait for etcd
to be healthy; Sign and World also wait for a successful migration. A failed
migration stops startup. Ctrl-C stops the supervisor in reverse dependency order.
The MHF launcher is disabled by default and can be started manually in the TUI.

Default ports are etcd `2379`/`2380`, Sign TCP `53000`, Sign HTTP `53001`, Entrance
`53002`, World lands `54001`/`54002`, and the Process Compose control API `8080`.
They are not automatically allocated. SQLite and etcd data live in `.state/`;
direnv caches live in `.direnv/`. Set `development.stateDirectory` in the local module to use
another data directory, then reload the shell. All three database URLs default
to the same SQLite file.

The common Nix option `development.stateDirectory` defaults to `.state` and accepts
an absolute path or a path relative to the working tree. The shared initialization
exports `PROJECT_ROOT` and resolves `PROJECT_STATE` at runtime, so paths never
point into the flake's Nix-store source copy. Modules read
`config.development.stateDirectory`; an internal helper handles runtime path resolution
and shell quoting for Wine, SQLite and etcd.
`PROJECT_STATE` is an exported result, not a configuration input.
Application configuration variables such as `SHRIMPMAN_SIGN__DATABASE__URL` retain
the names expected by the Rust services.

## Local module

Create `local/default.nix` for machine-specific modules. The entire `local/`
directory is ignored by Git, and the module is loaded only when it exists:

```nix
{ pkgs, mkCommand, ... }: {
  development.stateDirectory = ".state";
  development.ports.signHttp = 53011;
  shrimpman.sign.auto_sign_up = false;
  mhf.screen.window_resolution = { width = 1280; height = 720; };
  development.mhf = {
    gameDirectory = "/path/to/mhf";
    runner = "wine";
  };

  development.packages = [ pkgs.ripgrep ];
  settings.processes.shrimpman-sign.shutdown.timeout_seconds = 20;
  development.commands.hello = mkCommand {
    name = "hello";
    text = ''
      echo "Hello from the local module"
    '';
  };
}
```

`local` is a full module: it can import other modules, add packages and commands,
set `development.environment` defaults and `development.shellHook` code, and override
process-compose-flake's `settings`, `defaults` and `cli` options. Shared values
use `lib.mkDefault` where appropriate; use `lib.mkForce` to replace an option
already set at normal priority. Lists merge using the Nix module system.

Direnv automatically loads the optional local module. To include it when running
Nix commands manually, use `--impure` from the repository root:

```sh
nix develop --impure
nix run --impure .#mhf-launcher
```

Git flakes omit ignored files. The flake therefore reads the local module from
the absolute working-tree path (`PROJECT_ROOT`, or `PWD` when unset), available
only with impure evaluation. Plain `nix develop` / `nix run` use shared modules;
CI can evaluate the flake without a local module. No placeholder module or local
input is needed, and local edits never change `flake.lock`.

Ports are typed options under `development.ports`: `etcdClient`, `etcdPeer`,
`signTcp`, `signHttp`, `entranceTcp`, `worldLand1` and `worldLand2`.
Services, readiness probes and the launcher read these options directly; the
old port helper environment variables are no longer used. Existing environment
variables still take precedence over `development.environment` defaults for application inputs.
Nix modules are copied into the Nix store; supply credentials through the runtime
environment instead of placing them in module source.

The launcher uses a writable copy of the generated MHF configuration and the Sign HTTP port above.
`MHF_CONFIG` is relative to `mhf/`; `development.mhf.gameDirectory` is absolute or
relative to the repository root. Runner paths outside `PATH` should be absolute.
`MHF_SIGN__HTTP__BASE_URL` overrides the Sign URL.

All supported Nix hosts use Cargo Xwin. Execution is selected separately:

| Host | Build | Execution |
| --- | --- | --- |
| macOS / Linux | Cargo Xwin with `--xwin-arch x86` | Wine |
| WSL with Windows interoperability enabled | Cargo Xwin with `--xwin-arch x86` | Direct EXE execution, with paths converted by `wslpath` |

`development.mhf.runner` selects a Wine executable; an empty string skips Wine.
With its default `null`, the launcher detects WSL interoperability and falls
back to `wine` on other hosts. The WSL native path also forwards `MHF_*` variables
through `WSLENV`, preserving existing forwarding rules. Wine defaults to `$PROJECT_STATE/wine` (`.state/wine` with the default state directory).
Cargo reuses unchanged build artifacts.

The devShell initializes the shared environment once. Commands only select their
working directory and run; Process Compose inherits the shell environment.
Only `mhf-launcher` prepares the writable MHF configuration and default `WINEPREFIX`.
Entering the shell or building either workspace does not create MHF runtime files.
Compiler libraries belong in `development.buildInputs`.

The flake currently exposes only macOS/Linux outputs; WSL uses Linux outputs.
Native Windows uses Cargo and the EXE directly as shown in the launcher README;
this does not add native Windows support to Nix.

## Generated application configuration

Nix modules are the source of truth for both TOML files. Override
`shrimpman` and `mhf` in `local/default.nix`; nested attributes
merge, while default lists such as `shrimpman.world.lands` can be replaced.

Run `nix run .#update-configs` after changing public defaults to refresh
`shrimpman/config.toml` and `mhf/mhf.toml`. This command always excludes the local
module, even with `--impure`. Review and commit those generated files with the
module changes. `nix flake check` checks for drift and can be run in CI.

Nix launches services using `PROJECT_CONFIG`, pointing to the generated TOML.
Outside Nix, services still read `config.toml` in the current directory.
Database URLs default to the shared state directory; create `../.state` before
running directly from `shrimpman/`. Explicit application environment overrides
remain supported.

The MHF launcher writes game settings back to its configuration. Its generated
configuration is therefore copied into `$PROJECT_STATE/config/` with a filename
derived from the Nix store artifact. Launches reuse that writable copy. A changed
Nix configuration selects a new copy; previous copies remain intact. Export
`MHF_CONFIG` to use a separately managed file. The committed default MHF file is
for use outside Nix; copy it before running if you want to preserve it unchanged.

Translation hooks are disabled in the public MHF defaults (no `translation`
section). To enable Chinese translation locally, set
`mhf.translation = { locale = "zh-CN"; missing = "key"; };` in
`local/default.nix`.
