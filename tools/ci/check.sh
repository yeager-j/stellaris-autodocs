#!/usr/bin/env bash
# Every gate CI runs *on this machine's platform*, in the same order. A change is complete
# only when this exits 0 (AGENTS.md: complete the feedback loop).
#
# What a green run here does NOT cover: CI runs the Rust gates on a matrix of macos-latest
# and windows-latest (.github/workflows/ci.yml), and the Windows leg is the only thing that
# compiles and executes the `#[cfg(windows)]` arm of `src-tauri/src/durability.rs`. Running
# this script on macOS says nothing about that arm — deliberately stated, because reading a
# Win32 contract wrongly inside a branch no local gate compiles is a defect this project has
# already shipped once (docs/decision-log.md, D-123).
set -euo pipefail
cd "$(dirname "$0")/../.."

(cd src-tauri && cargo fmt --check)
(cd src-tauri && cargo clippy --all-targets --features test-support -- -D warnings)
(cd src-tauri && cargo test --features test-support)
npm run build
npm test
