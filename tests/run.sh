#!/usr/bin/env bash
# run.sh — Test runner for VM-based end-to-end tests.
#
# Usage:
#   ./tests/run.sh                              # run all test-*.sh
#   ./tests/run.sh test-fresh-install            # run one test
#   SETUP_EXE=./demo-app-setup.exe ./tests/run.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"

# Validate setup exe exists
if [[ ! -f "$SETUP_EXE" ]]; then
    echo "ERROR: Setup executable not found: ${SETUP_EXE}" >&2
    echo "Set SETUP_EXE to the path of your demo-app-setup.exe" >&2
    exit 1
fi

# Find the VM disk once upfront
vm_find_disk

# Discover tests
if [[ $# -gt 0 ]]; then
    # Run specific tests
    TESTS=()
    for arg in "$@"; do
        test_file="${SCRIPT_DIR}/${arg}.sh"
        if [[ ! -f "$test_file" ]]; then
            test_file="${SCRIPT_DIR}/test-${arg}.sh"
        fi
        if [[ ! -f "$test_file" ]]; then
            echo "ERROR: Test not found: ${arg}" >&2
            exit 1
        fi
        TESTS+=("$test_file")
    done
else
    # Run all test-*.sh files
    TESTS=()
    for f in "${SCRIPT_DIR}"/test-*.sh; do
        [[ -f "$f" ]] && TESTS+=("$f")
    done
fi

if [[ ${#TESTS[@]} -eq 0 ]]; then
    echo "No tests found."
    exit 0
fi

echo "========================================"
echo "Running ${#TESTS[@]} test(s)"
echo "Setup exe: ${SETUP_EXE}"
echo "VM: ${VM_NAME}"
echo "========================================"
echo ""

TOTAL=0
PASSED=0
FAILED=0
FAILED_NAMES=()

for test_file in "${TESTS[@]}"; do
    test_name="$(basename "$test_file" .sh)"
    TOTAL=$(( TOTAL + 1 ))

    echo "----------------------------------------"
    echo "TEST: ${test_name}"
    echo "----------------------------------------"

    # Reset assertion counters
    ASSERT_PASS=0
    ASSERT_FAIL=0

    # Restore → Start → Wait
    vm_stop
    vm_restore_snapshot
    vm_start

    # Source the test and run test_main
    test_result=0
    (
        source "$test_file"
        test_main
    ) || test_result=$?

    # Stop VM
    vm_stop

    if [[ $test_result -eq 0 ]]; then
        echo "RESULT: PASS"
        PASSED=$(( PASSED + 1 ))
    else
        echo "RESULT: FAIL"
        FAILED=$(( FAILED + 1 ))
        FAILED_NAMES+=("$test_name")
    fi
    echo ""
done

echo "========================================"
echo "SUMMARY: ${PASSED}/${TOTAL} passed, ${FAILED} failed"
if [[ ${#FAILED_NAMES[@]} -gt 0 ]]; then
    echo "Failed tests:"
    for name in "${FAILED_NAMES[@]}"; do
        echo "  - ${name}"
    done
fi
echo "========================================"

exit "$FAILED"
