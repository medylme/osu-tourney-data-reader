target_dir := env("TARGET_DIR", "target")
dist_dir := env("DIST_DIR", "dist")
windows_target := "x86_64-pc-windows-gnu"

name := `cargo metadata --format-version 1 --no-deps 2>/dev/null | jq -r '.packages[0].name'`
version := `cargo metadata --format-version 1 --no-deps 2>/dev/null | jq -r '.packages[0].version'`

set dotenv-load

default:
    @just --list

# development

dev *args:
    cargo run -- {{args}}

clean:
    cargo clean
    cargo clean --target-dir "{{target_dir}}"
    rm -rf {{dist_dir}}

check:
    cargo check --target {{windows_target}}

lint:
    cargo clippy --target {{windows_target}} -- -W clippy::pedantic

fmt:
    cargo fmt

test:
    cargo test

ci: check lint test
    cargo fmt -- --check
    @echo "All CI checks passed"

pre-commit: fmt lint test
    @echo "Ready to commit"

# builds - dev

build:
    cargo build

build-win: _ensure-dist
    cargo build --target {{windows_target}} --target-dir "{{target_dir}}"
    cp "{{target_dir}}/{{windows_target}}/debug/{{name}}.exe" "{{dist_dir}}/debug/{{name}}-windows-x86_64-debug.exe"

# builds - release

_ensure-dist:
    mkdir -p "{{dist_dir}}/debug"
    mkdir -p "{{dist_dir}}/release"

dist-win: _ensure-dist
    cargo build --target {{windows_target}} --target-dir "{{target_dir}}" --release
    cp "{{target_dir}}/{{windows_target}}/release/{{name}}.exe" "{{dist_dir}}/release/{{name}}-{{version}}-windows-x86_64.exe"
    @echo "Built: {{dist_dir}}/{{name}}-windows-x86_64.exe"

dist: dist-win
    cd "{{dist_dir}}/release" && sha256sum {{name}}-{{version}}-windows-x86_64.exe > {{name}}-{{version}}-windows-x86_64.exe.sha256
    @echo "Generated checksums:"
    @echo "  {{dist_dir}}/release/{{name}}-{{version}}-windows-x86_64.exe.sha256"
    @echo "Release builds completed in {{dist_dir}}/release"