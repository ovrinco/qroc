#!/usr/bin/env bash
set -euo pipefail
[[ $# == 4 ]] || exit 2
release=$1
export QROC_RESOURCE_ROOT=$release QROC_PHOD_ADMITTED_BIN="$release/runtime/phod/bin/phod-admitted" QROC_PHOD_ADMITTED_SHA256=$(cat "$release/runtime/phod/admitted.sha256")
case $2 in
 journey) exec python3 "$(dirname -- "$0")/phod-journey.py" "$release/bin/qroc-bridge" "$3" "$4" ;;
 unavailable) exec python3 "$(dirname -- "$0")/phod-unavailable.py" "$release/bin/qroc-bridge" "$3" "$4" ;;
 recovery) exec python3 "$(dirname -- "$0")/phod-recovery.py" "$release/bin/qroc-bridge" "$3" "$4" ;;
esac
