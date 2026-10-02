#!/usr/bin/env bash
test_main() {
    vm_prepare_setup
    vm_run_setup
    assert_file_exists "${WIN_INSTALL_DIR}\\.twi-meta.json"
    # Deliberately inherit the install directory to catch Windows cwd locks.
    vm_run_uninstall
    assert_uninstall_cleanup
    assert_registry_key_not_exists "$WIN_REGISTRY_KEY"
}
