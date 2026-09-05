set dotenv-load := true

# List available tasks.
default:
    @just --list

mhf_target := "i686-pc-windows-msvc"
mhf_executable := justfile_directory() + "/mhf/target/" + mhf_target + "/release/mhf-launcher.exe"
mhf_cargo := if os() == "windows" { "cargo" } else { "PATH=\"" + justfile_directory() + "/mhf/crates/mhf-launcher/tools:$PATH\" RANLIB_i686_pc_windows_msvc=llvm-ranlib cargo xwin" }
mhf_xwin_args := if os() == "windows" { "" } else { "--xwin-arch x86" }
mhf_wine := env("MHF_WINE", "wine")
mhf_runner := if os() == "windows" { "" } else { "\"" + mhf_wine + "\"" }
mhf_game_dir := env("MHF_GAME_DIR")

# Build the 32-bit Windows MHF launcher.
mhf-build:
    cd mhf && {{ mhf_cargo }} build -p shrimpman-mhf-launcher --release --target {{ mhf_target }} {{ mhf_xwin_args }}

# Build and launch MHF. Relative config paths are resolved from the workspace root.
mhf-launch config="mhf/mhf.toml": mhf-build
    {{ mhf_runner }} "{{ mhf_executable }}" --config "{{ config }}" --game-dir "{{ mhf_game_dir }}"

# Run the database management CLI.
db *args:
    cd shrimpman && cargo run -p shrimpman-persistence --bin toasty -- {{ args }}

# Run the Entrance service.
entrance:
    cd shrimpman && cargo run -p shrimpman-entrance

# Run the Sign service.
sign:
    cd shrimpman && cargo run -p shrimpman-sign

# Run the World service.
world:
    cd shrimpman && cargo run -p shrimpman-world
