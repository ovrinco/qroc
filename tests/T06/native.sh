#!/usr/bin/env bash
set -euo pipefail
if [[ $# != 4 || $1 != /* || $2 != /* || $3 != /* || $4 != /* ]]; then
 echo 'Usage: native.sh ABSOLUTE_BRIDGE ABSOLUTE_ROOT ABSOLUTE_INPUT ABSOLUTE_FRESH_CONFIG' >&2; exit 2
fi
candidate=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
[[ ! -e $4 ]] || { echo 'Native config must be fresh' >&2;exit 2; }
mkdir -p "$4/tests/T06"
cp -a "$candidate/ui" "$4/ui"
# Test wrapper only: select this owned helper without modifying candidate shell.
sed -i 's@tests/T01/NativeSmoke.qml@tests/T06/NativeSmoke.qml@g' "$4/ui/shell.qml"
cp "$candidate/tests/T06/NativeSmoke.qml" "$4/tests/T06/NativeSmoke.qml"
printf '%s\n' 'import "ui"' 'StandaloneShell {}' > "$4/shell.qml"
printf '%s\n' 'StandaloneShell 1.0 shell.qml' >> "$4/ui/qmldir"
export QROC_BRIDGE=$1 QROC_ROOT=$2 QROC_SMOKE_INPUT=$3 QROC_NATIVE_SMOKE=1
echo "T06_NATIVE_CGROUP $(cat /proc/self/cgroup)"
/usr/bin/qs --no-duplicate --path "$4/shell.qml" 2>&1 | tee "$4/journey.log"
rg -q 'T06_NATIVE_PASS' "$4/journey.log"
! rg -q 'T06_.*FAIL|ReferenceError|TypeError' "$4/journey.log"
