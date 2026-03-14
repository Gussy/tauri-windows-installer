#!/usr/bin/env bash
# test-overwrite-repair.sh — Install twice, verify overwrite works.

test_main() {
    echo "Pushing setup executable to VM..."
    vm_push "$SETUP_EXE" "${WIN_TEMP_DIR}\\setup.exe"

    echo "Running first install with --silent..."
    vm_exec cmd /c "${WIN_TEMP_DIR}\\setup.exe" --silent

    echo "Verifying first installation..."
    assert_dir_exists "${WIN_INSTALL_DIR}"
    assert_registry_key_exists "${WIN_REGISTRY_KEY}"

    echo "Running second install with --silent (overwrite)..."
    vm_exec cmd /c "${WIN_TEMP_DIR}\\setup.exe" --silent

    echo "Verifying installation still intact after overwrite..."
    assert_dir_exists "${WIN_INSTALL_DIR}"
    assert_file_exists "${WIN_INSTALL_DIR}\\${APP_EXE}"
    assert_registry_key_exists "${WIN_REGISTRY_KEY}"
    assert_registry_value "${WIN_REGISTRY_KEY}" "DisplayName" "${APP_NAME}"
}
