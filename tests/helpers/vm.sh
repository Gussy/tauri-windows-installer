#!/usr/bin/env bash
# vm.sh — VM lifecycle functions using UTM and qemu-img.

VM_DISK=""

# Locate the qcow2 disk image inside the UTM bundle.
vm_find_disk() {
    local utm_dirs=(
        "${HOME}/Library/Containers/com.utmapp.UTM/Data/Documents"
        "${HOME}/Documents/UTM"
    )

    for dir in "${utm_dirs[@]}"; do
        local bundle="${dir}/${VM_NAME}.utm"
        if [[ -d "$bundle" ]]; then
            local disk
            disk=$(find "$bundle" -name "*.qcow2" -print -quit 2>/dev/null)
            if [[ -n "$disk" ]]; then
                VM_DISK="$disk"
                return 0
            fi
        fi
    done

    echo "ERROR: Could not find qcow2 disk for VM '${VM_NAME}'" >&2
    return 1
}

# Restore the VM disk to a clean snapshot.
vm_restore_snapshot() {
    if [[ -z "$VM_DISK" ]]; then
        vm_find_disk
    fi
    echo "Restoring snapshot '${SNAPSHOT_NAME}'..."
    qemu-img snapshot -a "$SNAPSHOT_NAME" "$VM_DISK"
}

# Create a baseline snapshot of the current disk state.
vm_create_snapshot() {
    if [[ -z "$VM_DISK" ]]; then
        vm_find_disk
    fi
    echo "Creating snapshot '${SNAPSHOT_NAME}'..."
    qemu-img snapshot -c "$SNAPSHOT_NAME" "$VM_DISK"
}

# Start the VM via utmctl.
vm_start() {
    echo "Starting VM '${VM_NAME}'..."
    utmctl start "$VM_NAME" 2>/dev/null || true
    vm_wait_ready
}

# Stop the VM via utmctl (tolerant if already stopped).
vm_stop() {
    echo "Stopping VM '${VM_NAME}'..."
    utmctl stop "$VM_NAME" 2>/dev/null || true
    sleep 2
}

# Wait for the VM to become responsive (120s timeout).
vm_wait_ready() {
    local timeout="${VM_READY_TIMEOUT:-120}"
    local elapsed=0
    local interval=5

    echo "Waiting for VM to become ready (timeout: ${timeout}s)..."
    while (( elapsed < timeout )); do
        if utmctl exec "$VM_NAME" -- cmd /c "echo ready" >/dev/null 2>&1; then
            echo "VM is ready."
            return 0
        fi
        sleep "$interval"
        elapsed=$(( elapsed + interval ))
    done

    echo "ERROR: VM did not become ready within ${timeout}s" >&2
    return 1
}

# Execute a command inside the VM.
vm_exec() {
    utmctl exec "$VM_NAME" -- "$@"
}

# Execute a PowerShell command inside the VM.
vm_exec_ps() {
    utmctl exec "$VM_NAME" -- powershell -NoProfile -Command "$1"
}

# Push a local file to the VM.
vm_push() {
    local local_path="$1"
    local remote_path="$2"
    utmctl file push "$VM_NAME" "$local_path" "$remote_path"
}

# Pull a remote file from the VM.
vm_pull() {
    local remote_path="$1"
    local local_path="$2"
    utmctl file pull "$VM_NAME" "$remote_path" "$local_path"
}

# Handle the snapshot-create subcommand when invoked directly.
if [[ "${1:-}" == "snapshot-create" ]]; then
    vm_find_disk
    vm_create_snapshot
fi
