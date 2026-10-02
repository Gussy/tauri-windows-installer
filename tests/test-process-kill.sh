#!/usr/bin/env bash
test_main() {
    vm_prepare_setup
    vm_run_setup
    local old_pid
    old_pid=$(vm_exec_ps "\$p = Start-Process -FilePath $(ps_quote "${WIN_INSTALL_DIR}\\${APP_EXE}") -WorkingDirectory $(ps_quote "$WIN_INSTALL_DIR") -PassThru; Start-Sleep -Seconds 2; if (\$p.HasExited) { throw 'Application exited before replacement test' }; \$p.Id" | tr -d '\r')
    [[ "$old_pid" =~ ^[0-9]+$ ]] || { echo "Invalid old PID: $old_pid" >&2; return 1; }
    vm_run_setup
    assert_pid_not_running "$old_pid"
    assert_file_exists "${WIN_INSTALL_DIR}\\${APP_EXE}"
    assert_registry_value "$WIN_REGISTRY_KEY" DisplayVersion "$APP_VERSION"
}
