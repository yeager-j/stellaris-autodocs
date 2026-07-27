#!/usr/bin/env bash
# Every gate CI runs, in the same order. A change is complete only when this exits 0
# (AGENTS.md: complete the feedback loop).
set -euo pipefail
cd "$(dirname "$0")/../.."

(cd src-tauri && cargo fmt --check)
(cd src-tauri && cargo clippy --all-targets --features test-support -- -D warnings)
(cd src-tauri && cargo test --features test-support)
npm run build
npm test
