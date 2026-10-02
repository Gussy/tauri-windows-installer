#!/usr/bin/env bash
VM_DISK="${VM_DISK:-}"
VM_NAME="${VM_NAME:-windows-11-test}"
SNAPSHOT_NAME="${SNAPSHOT_NAME:-clean-baseline}"
UTMCTL="${UTMCTL:-$(command -v utmctl || true)}"
if [[ -z "$UTMCTL" && -x /Applications/UTM.app/Contents/MacOS/utmctl ]]; then
    UTMCTL=/Applications/UTM.app/Contents/MacOS/utmctl
fi
utmctl() {
    [[ -n "$UTMCTL" ]] || { echo 'UTM CLI not found; set UTMCTL.' >&2; return 1; }
    "$UTMCTL" "$@"
}
vm_require_tools() {
    [[ -n "$UTMCTL" && -x "$UTMCTL" ]] || { echo 'UTM CLI missing; set UTMCTL.' >&2; return 1; }
    local tool
    for tool in qemu-img iconv base64; do
        command -v "$tool" >/dev/null || { echo "Required tool missing: $tool" >&2; return 1; }
    done
}
vm_find_disk() {
    if [[ -n "$VM_DISK" ]]; then
        [[ -f "$VM_DISK" ]] || { echo "VM_DISK does not exist: $VM_DISK" >&2; return 1; }; return
    fi
    local dir bundle disk
    local disks=()
    for dir in "${HOME}/Library/Containers/com.utmapp.UTM/Data/Documents" "${HOME}/Documents/UTM"; do
        bundle="${dir}/${VM_NAME}.utm"
        [[ -d "$bundle" ]] || continue
        while IFS= read -r disk; do disks+=("$disk"); done < <(find "$bundle" -name '*.qcow2' -type f)
    done
    [[ ${#disks[@]} -eq 1 ]] || { echo "Expected one VM disk; found ${#disks[@]}. Set VM_DISK explicitly." >&2; return 1; }
    VM_DISK="${disks[0]}"
    export VM_DISK
}
vm_status() { utmctl status "$VM_NAME" | tr -d '\r' | tr '[:upper:]' '[:lower:]'; }
vm_assert_stopped() {
    local status
    status=$(vm_status) || return
    [[ "$status" == stopped ]] || { echo "Refusing snapshot operation: VM is $status." >&2; return 1; }
}
vm_restore_snapshot() { vm_assert_stopped || return; vm_find_disk || return; qemu-img snapshot -a "$SNAPSHOT_NAME" "$VM_DISK"; }
vm_create_snapshot() { vm_assert_stopped || return; vm_find_disk || return; qemu-img snapshot -c "$SNAPSHOT_NAME" "$VM_DISK"; }
vm_start() {
    local status
    status=$(vm_status) || return
    if [[ "$status" == stopped ]]; then utmctl start "$VM_NAME" || return; fi
    vm_wait_ready
}
vm_stop() {
    local status elapsed=0
    status=$(vm_status) || return
    if [[ "$status" != stopped ]]; then utmctl stop "$VM_NAME" --request || return; fi
    while (( elapsed < ${VM_STOP_TIMEOUT:-120} )); do
        status=$(vm_status) || return
        [[ "$status" == stopped ]] && return 0
        sleep 2; elapsed=$((elapsed + 2))
    done
    echo 'VM did not stop; snapshot remains untouched.' >&2
    return 1
}
vm_wait_ready() {
    local elapsed=0
    while (( elapsed < ${VM_READY_TIMEOUT:-120} )); do
        if utmctl exec "$VM_NAME" --cmd cmd.exe /c 'echo ready' >/dev/null 2>&1; then return 0; fi
        sleep 5; elapsed=$((elapsed + 5))
    done
    echo 'VM did not become ready.' >&2; return 1
}
vm_exec() { utmctl exec "$VM_NAME" --cmd "$@"; }
_ps_encoded() { printf '%s' "$1" | iconv -f UTF-8 -t UTF-16LE | base64 | tr -d '\n'; }
vm_exec_ps_raw() { vm_exec powershell.exe -NoProfile -NonInteractive -EncodedCommand "$(_ps_encoded "\$ErrorActionPreference='Stop'; $1")"; }
# Guest-agent commands run as SYSTEM. Use an interactive Limited task for the
# logged-in standard account; native exit codes and output are preserved.
vm_exec_ps() {
    vm_push "${SCRIPT_DIR}/helpers/run-as-user.ps1" 'C:\temp\twi-e2e-run-as-user.ps1'
    vm_exec powershell.exe -NoProfile -NonInteractive -File 'C:\temp\twi-e2e-run-as-user.ps1' -User "$VM_TEST_USER" -EncodedCommand "$(_ps_encoded "\$ErrorActionPreference='Stop'; $1")" -TimeoutSeconds "${VM_COMMAND_TIMEOUT:-180}"
}
vm_push() { utmctl file push "$VM_NAME" "$2" < "$1"; }
vm_pull() { utmctl file pull "$VM_NAME" "$1" > "$2"; }
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    set -euo pipefail
    vm_require_tools
    case "${1:-}" in
        snapshot-create) vm_create_snapshot ;;
        start) vm_start ;;
        stop) vm_stop ;;
        *) echo "Usage: $0 {snapshot-create|start|stop}" >&2; exit 2 ;;
    esac
fi
