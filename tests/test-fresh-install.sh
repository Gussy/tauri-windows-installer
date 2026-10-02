#!/usr/bin/env bash
test_main() {
    vm_prepare_setup
    vm_run_setup
    assert_dir_exists "$WIN_INSTALL_DIR"
    assert_file_exists "${WIN_INSTALL_DIR}\\${APP_EXE}"
    assert_file_exists "${WIN_INSTALL_DIR}\\.twi-meta.json"
    assert_registry_key_exists "$WIN_REGISTRY_KEY"
    assert_registry_value "$WIN_REGISTRY_KEY" DisplayName "$APP_NAME"
    assert_registry_value "$WIN_REGISTRY_KEY" DisplayVersion "$APP_VERSION"
    assert_registry_value "$WIN_REGISTRY_KEY" UninstallString "\"${WIN_INSTALL_DIR}\\${APP_EXE}\" --uninstall"
}
