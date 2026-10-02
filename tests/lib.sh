#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SETUP_EXE="${SETUP_EXE:-${PROJECT_ROOT}/demo-app-setup.exe}"
VM_NAME="${VM_NAME:-windows-11-test}"
SNAPSHOT_NAME="${SNAPSHOT_NAME:-clean-baseline}"
VM_TEST_USER="${VM_TEST_USER:-twi-test}"
APP_IDENTIFIER="${APP_IDENTIFIER:-com.gussy.demo-app}"
APP_NAME="${APP_NAME:-demo-app}"
APP_VERSION="${APP_VERSION:-0.0.0}"
APP_EXE="${APP_EXE:-demo-app.exe}"
WIN_TEMP_DIR="${WIN_TEMP_DIR:-C:\\temp\\twi-e2e}"
WIN_INSTALL_DIR="${WIN_INSTALL_DIR:-}"
WIN_DATA_DIR="${WIN_DATA_DIR:-}"
export WIN_REGISTRY_KEY="HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${APP_IDENTIFIER}"
source "${SCRIPT_DIR}/helpers/vm.sh"
source "${SCRIPT_DIR}/helpers/assert.sh"

ps_quote() { local value="${1//\'/\'\'}"; printf "'%s'" "$value"; }
vm_resolve_paths() {
    local appdata
    appdata=$(vm_exec_ps '[Environment]::GetFolderPath("LocalApplicationData")' | tr -d '\r')
    [[ "$appdata" =~ ^[A-Za-z]:\\ ]] || { echo "Invalid guest LocalAppData: $appdata" >&2; return 1; }
    WIN_INSTALL_DIR="${appdata}\\Programs\\${APP_IDENTIFIER}"
    WIN_DATA_DIR="${appdata}\\${APP_IDENTIFIER}"
    export WIN_INSTALL_DIR WIN_DATA_DIR
}
vm_assert_standard_user() {
    # These expressions deliberately expand in the guest's PowerShell.
    # shellcheck disable=SC2016
    vm_exec_ps '
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        $principal = [Security.Principal.WindowsPrincipal]::new($identity)
        if ($identity.IsSystem -or $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            throw "Tests must execute as a standard user, not SYSTEM or an elevated administrator."
        }
        if ((Get-ItemProperty "HKLM:\Software\Microsoft\Windows\CurrentVersion\Policies\System").EnableLUA -ne 1) { throw "UAC must be enabled." }
        if (-not (Get-MpComputerStatus -ErrorAction Stop).RealTimeProtectionEnabled) { throw "Defender must be enabled." }
    '
}
vm_prepare_setup() {
    vm_exec_ps_raw "New-Item -ItemType Directory -Path $(ps_quote "$WIN_TEMP_DIR") -Force | Out-Null; & icacls.exe $(ps_quote "$WIN_TEMP_DIR") /grant $(ps_quote "${VM_TEST_USER}:(OI)(CI)M") | Out-Null; if (\$LASTEXITCODE -ne 0) { exit \$LASTEXITCODE }"
    vm_push "$SETUP_EXE" "${WIN_TEMP_DIR}\\setup.exe"
}
vm_run_setup() {
    vm_exec_ps "\$p = Start-Process -FilePath $(ps_quote "${1:-${WIN_TEMP_DIR}\\setup.exe}") -ArgumentList '--silent','--no-launch' -Wait -PassThru; exit \$p.ExitCode"
}
vm_run_uninstall() {
    vm_exec_ps "\$p = Start-Process -FilePath $(ps_quote "${WIN_INSTALL_DIR}\\${APP_EXE}") -ArgumentList '--uninstall','--silent' -WorkingDirectory $(ps_quote "$WIN_INSTALL_DIR") -Wait -PassThru; exit \$p.ExitCode"
}
