# List available tasks.
default:
    @just --list

# Run the database management CLI.
db *args:
    cargo run -p shrimpman-persistence --bin toasty -- {{ args }}

# Run the Entrance service.
entrance:
    cargo run -p shrimpman-entrance

# Run the Sign service.
sign:
    cargo run -p shrimpman-sign
