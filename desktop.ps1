# Launch the native desktop analyzer (Tauri).
#
#   .\desktop.ps1            # build + run the windowed app (drag a .age3Yrec onto it)
#
# The frontend is the same self-contained viewer/index.html; the Rust backend
# (src-tauri) reuses the CLI parser as a library. No Node build step.
#
# For distributable installers (.msi/.exe) instead of a dev run, install the
# Tauri CLI once and bundle:
#   cargo install tauri-cli --version "^2"
#   cargo tauri build            # output under src-tauri/target/release/bundle/

$ErrorActionPreference = "Stop"
$repo = $PSScriptRoot
$manifest = Join-Path $repo "src-tauri\Cargo.toml"

Write-Host "Building and launching AoE3 Replay Analyzer (desktop)..."
& cargo run --release --manifest-path $manifest
