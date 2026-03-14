#!/usr/bin/env bash
# test-fresh-install.sh — Verify fresh installation on a clean machine.

test_main() {
    echo "Pushing setup executable to VM..."
    vm_push "$SETUP_EXE" "${WIN_TEMP_DIR}\\setup.exe"

    echo "Running installer with --silent..."
    vm_exec cmd /c "${WIN_TEMP_DIR}\\setup.exe" --silent

    echo "Verifying installation..."
    assert_dir_exists "${WIN_INSTALL_DIR}"
    assert_file_exists "${WIN_INSTALL_DIR}\\${APP_EXE}"
    assert_registry_key_exists "${WIN_REGISTRY_KEY}"
    assert_registry_value "${WIN_REGISTRY_KEY}" "DisplayName" "${APP_NAME}"
    assert_registry_value "${WIN_REGISTRY_KEY}" "DisplayVersion" "${APP_VERSION}"

    # Verify UninstallString contains --uninstall
    vm_exec_ps "
        \$val = (Get-ItemProperty -Path '${WIN_REGISTRY_KEY}' -Name 'UninstallString').UninstallString
        if (\$val -notlike '*--uninstall*') {
            Write-Host \"UninstallString does not contain --uninstall: \$val\"
            exit 1
        }
    "
    echo "  PASS: UninstallString contains --uninstall"
}
