#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"
[[ -f "$SETUP_EXE" ]] || { echo "Setup executable missing: $SETUP_EXE" >&2; exit 1; }
vm_require_tools
vm_find_disk
TESTS=()
if [[ $# -gt 0 ]]; then
    for name in "$@"; do
        [[ "$name" =~ ^(test-)?[a-z0-9-]+$ ]] || { echo "Invalid test name: $name" >&2; exit 2; }
        file="${SCRIPT_DIR}/${name}.sh"
        [[ -f "$file" ]] || file="${SCRIPT_DIR}/test-${name}.sh"
        [[ -f "$file" ]] || { echo "Test not found: $name" >&2; exit 2; }
        TESTS+=("$file")
    done
else
    for file in "${SCRIPT_DIR}"/test-*.sh; do [[ ! -f "$file" ]] || TESTS+=("$file"); done
fi
[[ ${#TESTS[@]} -gt 0 ]] || { echo 'No tests found.' >&2; exit 1; }
export SETUP_EXE VM_NAME VM_DISK SNAPSHOT_NAME VM_TEST_USER APP_IDENTIFIER APP_NAME APP_VERSION APP_EXE WIN_TEMP_DIR
failed=0
trap 'vm_stop || true' EXIT
for file in "${TESTS[@]}"; do
    echo "TEST: $(basename "$file" .sh)"
    vm_stop
    vm_restore_snapshot
    vm_start
    # The ||/if exemption exists only in this parent, never in the child shell.
    if bash "${SCRIPT_DIR}/run-one.sh" "$file"; then
        echo 'RESULT: PASS'
    else
        echo 'RESULT: FAIL'; failed=$((failed + 1))
    fi
    vm_stop
done
echo "SUMMARY: $((${#TESTS[@]} - failed))/${#TESTS[@]} passed, $failed failed"
[[ $failed -eq 0 ]]
