#!/usr/bin/env bash
ASSERT_PASS=0
ASSERT_FAIL=0
_assert_ps() {
    local description="$1" command="$2" code=0
    vm_exec_ps "$command" || code=$?
    if [[ $code -eq 0 ]]; then
        printf '  PASS: %s\n' "$description"; ASSERT_PASS=$((ASSERT_PASS + 1))
    else
        printf '  FAIL: %s\n' "$description" >&2; ASSERT_FAIL=$((ASSERT_FAIL + 1)); return "$code"
    fi
}
assert_file_exists() { _assert_ps "File exists: $1" "if (-not (Test-Path -LiteralPath $(ps_quote "$1") -PathType Leaf)) { exit 1 }"; }
assert_file_not_exists() { _assert_ps "File absent: $1" "if (Test-Path -LiteralPath $(ps_quote "$1")) { exit 1 }"; }
assert_dir_exists() { _assert_ps "Directory exists: $1" "if (-not (Test-Path -LiteralPath $(ps_quote "$1") -PathType Container)) { exit 1 }"; }
assert_dir_not_exists() { _assert_ps "Directory absent: $1" "if (Test-Path -LiteralPath $(ps_quote "$1")) { exit 1 }"; }
assert_registry_key_exists() { _assert_ps "Registry key exists: $1" "if (-not (Test-Path -LiteralPath $(ps_quote "$1"))) { exit 1 }"; }
assert_registry_key_not_exists() { _assert_ps "Registry key absent: $1" "if (Test-Path -LiteralPath $(ps_quote "$1")) { exit 1 }"; }
assert_registry_value() { _assert_ps "Registry value $2 = $3" "\$value = Get-ItemPropertyValue -LiteralPath $(ps_quote "$1") -Name $(ps_quote "$2"); if (\$value -ne $(ps_quote "$3")) { throw 'Unexpected registry value' }"; }
assert_process_running() { _assert_ps "Process running: $1" "if (-not (Get-Process -Name $(ps_quote "$1") -ErrorAction SilentlyContinue)) { exit 1 }"; }
assert_process_not_running() { _assert_ps "Process absent: $1" "if (Get-Process -Name $(ps_quote "$1") -ErrorAction SilentlyContinue) { exit 1 }"; }
assert_pid_not_running() {
    [[ "$1" =~ ^[0-9]+$ ]] || { echo 'Invalid process ID' >&2; return 1; }
    _assert_ps "Original PID exited: $1" "if (Get-Process -Id $1 -ErrorAction SilentlyContinue) { exit 1 }"
}
assert_uninstall_cleanup() {
    _assert_ps 'Uninstall cleanup completed' "
        \$root = $(ps_quote "$WIN_INSTALL_DIR"); \$parent = Split-Path -Parent \$root; \$leaf = Split-Path -Leaf \$root
        \$deadline = [DateTime]::UtcNow.AddSeconds(30)
        do {
            \$leftovers = @(Get-ChildItem -LiteralPath \$parent -Directory | Where-Object { \$_.Name -like (\$leaf + '_uninstalling_*') -or \$_.Name -like (\$leaf + '.twi-uninstall-*') })
            if (-not (Test-Path -LiteralPath \$root) -and \$leftovers.Count -eq 0 -and -not (Test-Path -LiteralPath (\$root + '.twi-uninstall.json'))) { exit 0 }
            Start-Sleep -Milliseconds 200
        } while ([DateTime]::UtcNow -lt \$deadline)
        throw 'Uninstall left an original or renamed directory'
    "
}
