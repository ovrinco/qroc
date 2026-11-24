#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 4 || $1 != /* || $2 != /* || $3 != /* || $4 != /* ]]; then
 echo 'Usage: native.sh ABSOLUTE_BRIDGE ABSOLUTE_ROOT ABSOLUTE_INPUT ABSOLUTE_FRESH_CONFIG' >&2; exit 2
fi
candidate=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
[[ ! -e $4 ]] || { echo 'Native config must be fresh' >&2;exit 2; }
mkdir -p "$4/tests/T01"
cp -a "$candidate/ui" "$4/ui"
cp "$candidate/tests/T01/NativeSmoke.qml" "$4/tests/T01/NativeSmoke.qml"
printf '%s\n' 'import "ui"' 'StandaloneShell {}' > "$4/shell.qml"
printf '%s\n' 'StandaloneShell 1.0 shell.qml' >> "$4/ui/qmldir"
export QROC_BRIDGE=$1 QROC_ROOT=$2 QROC_SMOKE_INPUT=$3 QROC_NATIVE_SMOKE=1
echo "T01_NATIVE_CGROUP $(cat /proc/self/cgroup)"
exec /usr/bin/qs --no-duplicate --path "$4/shell.qml"
