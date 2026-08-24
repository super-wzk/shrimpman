# List available tasks.
default:
    @just --list

# Run the database management CLI.
db *args:
    cargo run -p shrimpman-persistence --bin toasty -- {{ args }}

# Run the Sign service.
sign:
    cargo run -p shrimpman-sign
