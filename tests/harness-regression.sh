#!/usr/bin/env bash
# No VM required: verify failure propagation and exact UTM command contracts.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"
twi_tmp=$(mktemp -d)
trap 'rm -rf "$twi_tmp"' EXIT
cat > "$twi_tmp/provider.sh" <<'MOCK'
vm_exec_ps() { return "${TWI_MOCK_EXIT:-1}"; }
MOCK
cat > "$twi_tmp/failing.sh" <<'TEST'
test_main() {
    assert_file_exists 'C:\missing.exe'
    printf 'continued after failure\n'
}
TEST
if TWI_TEST_PROVIDER="$twi_tmp/provider.sh" bash "$SCRIPT_DIR/run-one.sh" "$twi_tmp/failing.sh" > "$twi_tmp/result" 2>&1; then
    echo 'Regression: failed assertion passed' >&2; exit 1
fi
if rg -q 'continued after failure' "$twi_tmp/result"; then echo 'Regression: test continued after failure' >&2; exit 1; fi
TWI_TEST_PROVIDER="$twi_tmp/provider.sh" TWI_MOCK_EXIT=0 bash "$SCRIPT_DIR/run-one.sh" "$twi_tmp/failing.sh" >/dev/null
cat > "$twi_tmp/command-failure.sh" <<'TEST'
test_main() { false; echo 'command incorrectly continued'; }
TEST
if TWI_TEST_PROVIDER="$twi_tmp/provider.sh" bash "$SCRIPT_DIR/run-one.sh" "$twi_tmp/command-failure.sh" >/dev/null; then
    echo 'Regression: command failure passed' >&2; exit 1
fi
# Functions replace external programs; these tests cannot alter a real VM.
utmctl() {
    printf '%s\n' "$@" > "$twi_tmp/args"
    case "$1 $2" in
        'file push') cat > "$twi_tmp/upload" ;;
        'file pull') printf 'remote bytes' ;;
        *) : ;;
    esac
}
printf 'local bytes' > "$twi_tmp/source"
vm_push "$twi_tmp/source" 'C:\remote path.exe'
[[ $(cat "$twi_tmp/upload") == 'local bytes' ]]
[[ $(sed -n '4p' "$twi_tmp/args") == 'C:\remote path.exe' ]]
[[ $(wc -l < "$twi_tmp/args") -eq 4 ]]
vm_pull 'C:\remote path.exe' "$twi_tmp/destination"
[[ $(cat "$twi_tmp/destination") == 'remote bytes' ]]
vm_exec cmd.exe /c 'echo ready'
[[ $(sed -n '3p' "$twi_tmp/args") == --cmd ]]
qemu-img() { echo 'snapshot unexpectedly touched' > "$twi_tmp/snapshot"; }
# Invoked indirectly by the sourced snapshot helper.
# shellcheck disable=SC2329
# Invoked indirectly by the sourced VM helper under test.
# shellcheck disable=SC2317
vm_status() { echo started; }
export VM_DISK="$twi_tmp/source"
if vm_restore_snapshot >/dev/null 2>&1; then echo 'Regression: live VM snapshot accepted' >&2; exit 1; fi
[[ ! -e "$twi_tmp/snapshot" ]]
vm_status() { echo stopped; }
vm_restore_snapshot
[[ -e "$twi_tmp/snapshot" ]]
[[ $(ps_quote "C:\a'b.exe") == "'C:\a''b.exe'" ]]
vm_exec_ps() { printf 'C:\Users\Test User\AppData\Local\r\n'; }
vm_resolve_paths
[[ "$WIN_INSTALL_DIR" == 'C:\Users\Test User\AppData\Local\Programs\com.gussy.demo-app' ]]
echo 'Harness regression tests passed.'
