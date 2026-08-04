#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

"$repo_root/scripts/build-web-worker.sh"
cd "$repo_root/crates/dzip-gui"
exec dx serve --web "$@"
