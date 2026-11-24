#!/usr/bin/env bash
set -euo pipefail
[[ $# == 3 && $1 == /* && $2 == /* && $3 == /* && ! -e $3 ]] || exit 2
candidate=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
mkdir -p "$3/tests/T06"
cp -a "$candidate/ui" "$3/ui"
cp "$candidate/tests/T06/SchemaSmoke.qml" "$candidate/tests/T06/FixtureTransport.qml" "$candidate/tests/T06/AdmittedFixtures.js" "$3/tests/T06/"
printf '%s\n' 'import "tests/T06"' 'SchemaSmoke {}' > "$3/shell.qml"
printf '%s\n' 'SchemaSmoke 1.0 SchemaSmoke.qml' 'FixtureTransport 1.0 FixtureTransport.qml' > "$3/tests/T06/qmldir"
export QROC_BRIDGE=$1 QROC_ROOT=$2 QROC_INPUT_IMAGE="$3/schema-fixture.png"
echo "T06_SCHEMA_FIXTURE_CGROUP $(cat /proc/self/cgroup)"
/usr/bin/qs --no-duplicate --path "$3/shell.qml" 2>&1 | tee "$3/journey.log"
rg -q 'T06_SCHEMA_FIXTURE_PASS' "$3/journey.log"
! rg -q 'T06_.*FAIL|ReferenceError|TypeError|FAIL!' "$3/journey.log"
