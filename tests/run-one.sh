#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"
if [[ -n "${TWI_TEST_PROVIDER:-}" ]]; then
    source "$TWI_TEST_PROVIDER"
else
    vm_assert_standard_user
    vm_resolve_paths
fi
source "$1"
test_main
[[ $ASSERT_FAIL -eq 0 ]] || exit 1
printf 'Assertions: %s passed\n' "$ASSERT_PASS"
