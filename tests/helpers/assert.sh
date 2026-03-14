#!/usr/bin/env bash
# assert.sh — Assertion helpers for VM tests.
# Each function runs a PowerShell command in the VM and checks the result.

ASSERT_PASS=0
ASSERT_FAIL=0

_assert_result() {
    local description="$1"
    local exit_code="$2"

    if [[ "$exit_code" -eq 0 ]]; then
        echo "  PASS: ${description}"
        ASSERT_PASS=$(( ASSERT_PASS + 1 ))
    else
        echo "  FAIL: ${description}"
        ASSERT_FAIL=$(( ASSERT_FAIL + 1 ))
        return 1
    fi
}

assert_file_exists() {
    local path="$1"
    vm_exec_ps "if (!(Test-Path '$path')) { exit 1 }"
    _assert_result "File exists: ${path}" $?
}

assert_file_not_exists() {
    local path="$1"
    vm_exec_ps "if (Test-Path '$path') { exit 1 }"
    _assert_result "File does not exist: ${path}" $?
}

assert_dir_exists() {
    local path="$1"
    vm_exec_ps "if (!(Test-Path '$path' -PathType Container)) { exit 1 }"
    _assert_result "Directory exists: ${path}" $?
}

assert_dir_not_exists() {
    local path="$1"
    vm_exec_ps "if (Test-Path '$path' -PathType Container) { exit 1 }"
    _assert_result "Directory does not exist: ${path}" $?
}

assert_registry_key_exists() {
    local key="$1"
    vm_exec_ps "if (!(Test-Path '$key')) { exit 1 }"
    _assert_result "Registry key exists: ${key}" $?
}

assert_registry_key_not_exists() {
    local key="$1"
    vm_exec_ps "if (Test-Path '$key') { exit 1 }"
    _assert_result "Registry key does not exist: ${key}" $?
}

assert_registry_value() {
    local key="$1"
    local name="$2"
    local expected="$3"
    vm_exec_ps "
        \$val = (Get-ItemProperty -Path '$key' -Name '$name' -ErrorAction Stop).'$name'
        if (\$val -ne '$expected') {
            Write-Host \"Expected '$expected', got '\$val'\"
            exit 1
        }
    "
    _assert_result "Registry value ${name} = ${expected}" $?
}

assert_process_running() {
    local name="$1"
    vm_exec_ps "if (!(Get-Process -Name '$name' -ErrorAction SilentlyContinue)) { exit 1 }"
    _assert_result "Process running: ${name}" $?
}

assert_process_not_running() {
    local name="$1"
    vm_exec_ps "if (Get-Process -Name '$name' -ErrorAction SilentlyContinue) { exit 1 }"
    _assert_result "Process not running: ${name}" $?
}
