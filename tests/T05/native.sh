#!/usr/bin/env bash
set -euo pipefail
[[ ( $# == 3 || $# == 5 ) && $1 == /* && $2 == /* && $3 == /* && ! -e $3 ]] || { echo 'Usage: native.sh BRIDGE FRESH_ROOT FRESH_CONFIG';exit 2; }
candidate=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
mkdir -p "$3/tests/T05"
cp -a "$candidate/ui" "$3/ui"
cp "$candidate/tests/T05/NativeJourney.qml" "$3/tests/T05/NativeJourney.qml"
printf '%s
' 'import "tests/T05"' 'NativeJourney {}' > "$3/shell.qml"
if [[ $# == 5 ]];then export QROC_PHOD_ADMITTED_BIN=$4 QROC_PHOD_ADMITTED_SHA256=$5;fi
export QROC_BRIDGE=$1 QROC_ROOT=$2 QROC_RESOURCE_ROOT=$candidate
printf 'T05_NATIVE_CONTEXT executable=%s root=%s qml=%s cgroup=%s\n' "$1" "$2" "$3/tests/T05/NativeJourney.qml" "$(cat /proc/self/cgroup)"
sha256sum "$1" "$3/ui/ModulePhod.qml" "$3/tests/T05/NativeJourney.qml"
exec /usr/bin/qs --no-duplicate --path "$3/shell.qml"
