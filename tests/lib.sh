#!/usr/bin/env bash
# lib.sh — Shared configuration and sourcing for VM tests.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Default path to the setup executable
SETUP_EXE="${SETUP_EXE:-${PROJECT_ROOT}/demo-app-setup.exe}"

# VM configuration
VM_NAME="${VM_NAME:-windows-11-test}"
SNAPSHOT_NAME="${SNAPSHOT_NAME:-clean-baseline}"

# App configuration (must match demo-app manifest)
APP_IDENTIFIER="${APP_IDENTIFIER:-com.gussy.demo-app}"
APP_NAME="${APP_NAME:-demo-app}"
APP_VERSION="${APP_VERSION:-0.0.0}"
APP_EXE="${APP_EXE:-demo-app.exe}"

# Windows paths (constructed from identifier)
WIN_INSTALL_DIR="\$env:LOCALAPPDATA\\${APP_IDENTIFIER}"
WIN_REGISTRY_KEY="HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${APP_IDENTIFIER}"

# Remote temp directory for test artifacts
WIN_TEMP_DIR="C:\\temp"

# Source helpers
source "${SCRIPT_DIR}/helpers/vm.sh"
source "${SCRIPT_DIR}/helpers/assert.sh"
