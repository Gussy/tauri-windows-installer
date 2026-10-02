#!/usr/bin/env bash
test_main() {
    vm_prepare_setup
    vm_run_setup
    assert_file_exists "${WIN_INSTALL_DIR}\\${APP_EXE}"
    vm_exec_ps "Remove-Item -LiteralPath $(ps_quote "${WIN_INSTALL_DIR}\\${APP_EXE}") -Force"
    vm_run_setup
    assert_file_exists "${WIN_INSTALL_DIR}\\${APP_EXE}"
    assert_file_exists "${WIN_INSTALL_DIR}\\.twi-meta.json"
    assert_registry_value "$WIN_REGISTRY_KEY" DisplayVersion "$APP_VERSION"
}
