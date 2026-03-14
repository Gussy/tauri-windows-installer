#!/usr/bin/env bash
# test-uninstall.sh — Install then uninstall, verify full cleanup.

test_main() {
    echo "Pushing setup executable to VM..."
    vm_push "$SETUP_EXE" "${WIN_TEMP_DIR}\\setup.exe"

    echo "Running installer with --silent..."
    vm_exec cmd /c "${WIN_TEMP_DIR}\\setup.exe" --silent

    echo "Verifying installation succeeded..."
    assert_dir_exists "${WIN_INSTALL_DIR}"
    assert_registry_key_exists "${WIN_REGISTRY_KEY}"

    echo "Running uninstaller..."
    vm_exec cmd /c "${WIN_INSTALL_DIR}\\${APP_EXE}" --uninstall

    # Wait for self-delete (uses delayed cmd.exe hack)
    echo "Waiting for uninstall cleanup..."
    sleep 5

    echo "Verifying uninstallation..."
    assert_dir_not_exists "${WIN_INSTALL_DIR}"
    assert_registry_key_not_exists "${WIN_REGISTRY_KEY}"
}
