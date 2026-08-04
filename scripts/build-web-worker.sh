#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
worker_crate="$repo_root/crates/dzip-worker"
asset_root="$repo_root/crates/dzip-gui/assets/worker"

rm -rf "$asset_root/pkg"
mkdir -p "$asset_root/pkg"

RUSTUP_TOOLCHAIN=stable \
wasm-pack build "$worker_crate" \
  --target web \
  --release \
  --no-opt \
  --out-dir "$asset_root/pkg" \
  --out-name dzip_gui_worker

rm -f "$asset_root/pkg/.gitignore"
