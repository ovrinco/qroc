#!/usr/bin/env bash
set -euo pipefail
[[ $# == 6 ]] || exit 2
export QROC_ROOT=$2 QROC_RELEASE_QA=1 QROC_QA_MODE=$3 QROC_QA_INPUT=$4 QROC_QA_AUDIO=$5 QROC_QA_REOPEN=$6
printf 'T08_NATIVE_CONTEXT launcher=%s root=%s mode=%s cgroup=%s\n' "$1" "$2" "$3" "$(cat /proc/self/cgroup)"
"$1" --identity
mkdir -p "$2"
log="$2/T08-native-$3-$6-$$.log"
"$1" 2>&1 | tee "$log"
rg -q T08_INSTALLED_PASS "$log"
! rg -q 'T08_INSTALLED_FAIL|T08_REQUEST_ERROR|ReferenceError|TypeError' "$log"
