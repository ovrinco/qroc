#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo"
cargo build --locked --bin qroc-bridge -j 2
target=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
roots=$(mktemp -d)
python3 tests/T305/fake_shim.py "$target/debug/qroc-bridge" "$roots"
