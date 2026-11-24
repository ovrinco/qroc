#!/usr/bin/env bash
set -euo pipefail
[[ $# == 2 && $1 == /* && $2 == /* ]] || { echo 'Usage: build.sh ABSOLUTE_CARGO_WRAPPER ABSOLUTE_PRIVATE_TARGET_ROOT';exit 2; }
runtime=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
export CARGO_BUILD_JOBS=2
CARGO_TARGET_DIR="$2/derive" "$1" build --offline --locked --manifest-path "$runtime/source/derive/Cargo.toml"
CARGO_TARGET_DIR="$2/semantic" "$1" build --offline --locked --manifest-path "$runtime/source/semantic/Cargo.toml"
install -m755 "$2/derive/debug/phod-local-derive" "$runtime/bin/phod-local-derive"
install -m755 "$2/semantic/debug/phod-semantic" "$runtime/bin/phod-semantic"
strip "$runtime/bin/phod-local-derive" "$runtime/bin/phod-semantic"
(cd "$runtime" && sha256sum bin/phod-local-derive bin/phod-semantic workbench.py owner.py > manifest.sha256)
