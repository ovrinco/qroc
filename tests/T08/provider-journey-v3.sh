#!/usr/bin/env bash
set -euo pipefail
[[ $# == 2 ]] || exit 2
: "${QROC_GATEWAY:?shared GPU proxy required}" "${QROC_MODEL:?bound model required}" "${QROC_MAX_TOKENS:?explicit token ceiling required}"
export QROC_TIMEOUT_SECONDS=45
exec python3 "$(dirname -- "$0")/provider-journey-v3.py" "$1" "$2"
