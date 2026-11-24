#!/usr/bin/env bash
set -euo pipefail
[[ $# == 5 && $1 == /* && $2 == /* && $3 == /* && $4 == /* ]] || exit 2
candidate=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
[[ ! -e $4 ]] || { echo 'Config must be fresh' >&2;exit 2; }
mkdir -p "$4/tests/T07"
cp -a "$candidate/ui" "$4/ui"
cp "$candidate/tests/T07/NativeCreative.qml" "$4/tests/T07/NativeCreative.qml"
printf '%s\n' 'CreativeWorkspace 1.0 CreativeWorkspace.qml' >> "$4/ui/qmldir"
printf '%s\n' 'import "tests/T07"' 'NativeCreative {}' > "$4/shell.qml"
export QROC_BRIDGE=$1 QROC_ROOT=$2 T07_INPUT=$3 T07_REOPEN=$5 T07_QML_IDENTITY="$4/ui/CreativeWorkspace.qml"
export T07_PREVIEW="$4/creative-preview.png"
export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software
echo "T07_NATIVE_CGROUP $(cat /proc/self/cgroup)"
sha256sum "$1" "$4/ui/CreativeWorkspace.qml" "$4/ui/Backend.qml" "$3"
exec /usr/bin/qs --no-duplicate --path "$4/shell.qml"
