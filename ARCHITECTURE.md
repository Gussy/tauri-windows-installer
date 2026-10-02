# Architecture

## Components

| Crate | Responsibility |
| --- | --- |
| `core` | Validated manifest / ownership types, shared operation lock, resource constants and optional bundling API |
| `bundler` | Tauri config parsing, CLI overrides, verified stub selection, WebView2 download and signing configuration |
| `installer` | Windows setup: prerequisites, validated staging, recoverable replacement, registration and launch |
| `uninstaller` | Linked into the application: metadata/root discovery, serialized uninstall and retryable self cleanup |

The default workspace members are core and bundler. Non-Windows tests do not substitute for the native Windows jobs. Build the installer first on Windows. `TWI_SETUP_EXE` explicitly chooses the artifact embedded by the bundler build script; `--setup-exe` overrides it at packaging time. `--stub-info` allows release automation to compare the embedded stub hash with the freshly built source artifact.

## Package protocol

The Windows PE resources include:

| Resource | Content |
| --- | --- |
| `TWI_RESOURCE` | Package identification marker |
| `TWI_MANIFEST` | `TWI-MANIFEST\n` followed by a JSON envelope with `format_version: 1` and `manifest` |
| `TWI_BUNDLE` | Zstd-compressed tar archive |
| `TWI_WEBVIEW2` | Optional WebView2 bootstrapper |
| `TWI_WEBVIEW2_FILENAME` | Validated bootstrapper filename |

The manifest contains product identity, relative executable path, desktop-shortcut policy, SHA256 of the compressed payload and expected unpacked bytes. The runtime rejects unsupported formats, unsafe filesystem values and inconsistent payloads. The core decoder retains explicit compatibility handling for the original binary manifest; new packages use JSON. A bare setup stub carries `TWI_SETUP_ABI_V1\0`, so incompatible or already-packaged stubs cannot silently become new packages.

PE resources are merged with the original stub resources. The final package must retain its `asInvoker` execution level. SHA256 binds manifest and payload against accidental corruption; publisher authenticity is supplied by Authenticode signing, not an unkeyed digest.

## Installation and uninstall

Managed binaries live beneath `%LOCALAPPDATA%\Programs\<identifier>`. Application data beneath `%LOCALAPPDATA%\<identifier>` remains through uninstall. Each setup invocation retains its diagnostic log in `%TEMP%\twi-installer-*.log`, including failures before product paths are available. Metadata is `.twi-meta.json` at the installation root, even when the main executable is nested.

An OS-backed application lock serializes setup and uninstall. Setup recovers a prior transaction before beginning another operation. It verifies prerequisites and available storage, extracts a bounded payload into an exclusive sibling staging directory and validates the executable before stopping / replacing the old installation. The transaction journal is `<identifier>.twi-transaction.json`; sibling stage and backup directories use `.twi-stage-` and `.twi-backup-` suffixes. Metadata, registration and the requested shortcut change must succeed before commit; failures restore the previous version. A requested application launch that cannot be spawned also rolls back. `--no-launch` skips that launch.

Uninstall discovers the root through validated ownership metadata, moves cwd outside the managed tree and shares the same lock. It retires binaries to `<identifier>.twi-uninstall-<nonce>`, writes `<identifier>.twi-uninstall.json`, and retains a standalone retry command until asynchronous cleanup completes. Its worker lives in `Programs\.twi-uninstall-workers`. Cleanup must remove the retired directory and retry registration; disappearance of the original path alone does not establish completion.

## Tests

`bash tests/harness-regression.sh` requires no VM. It proves that an early failed assertion or command cannot be hidden by later success, verifies the UTM CLI forms and refuses snapshot modification while the VM is running.

`tests/windows-e2e.ps1` uses a small native Rust example, `twi-test-app`, linked with the actual uninstaller. Build it using the main locked dependency graph:

```powershell
cargo build --locked --release -p twi_installer
cargo build --locked --release -p twi_uninstaller --example twi-test-app
$env:TWI_SETUP_EXE = (Resolve-Path target\release\setup.exe).Path
cargo build --locked --release -p twi_bundler
.\tests\windows-e2e.ps1
```

The suite checks package resource integrity and Windows manifest preservation, shared setup/uninstall locking, failed replacement with an open directory handle, corrupt payload rejection, interrupted-switch journal recovery, installer-launched cwd and PID termination, real version changes with removed/new sidecars, nested executables, user-data preservation, repair, shortcut changes and completed self-deletion. Cleanup includes deep/long assets and live/broken junctions; a locked-sidecar failure must retain a working standalone retry. It creates a unique product identifier and retains failed installation trees/logs for diagnosis. It needs internet access for an Evergreen bootstrapper when the runtime is absent.

The hosted Windows runner is an administrator account. Its native suite establishes runtime behavior but is not proof of standard-user behavior. Run `tests/windows-e2e.ps1 -RequireStandardUser` under a standard account, or use the VM suite below. Further coverage remains useful for low disk/quota limits, interruption at every transaction phase, ACL-denied registration and prerequisite network failures.

## UTM VM setup

On macOS, `brew bundle` installs UTM, QEMU, Packer, Task and ShellCheck. Supply verified Windows ARM64 media, its SHA256 and VirtIO guest-agent media:

```sh
task vm:build-image ISO_PATH=/absolute/windows.iso ISO_SHA256=<verified-hex> VIRTIO_ISO_PATH=/absolute/virtio.iso
```

Import the resulting disk into a UTM QEMU VM named `windows-11-test`. The Packer fixture leaves UAC, Defender and Windows Update defaults enabled. It provisions through an administrator, then auto-logs in the standard `twi-test` account. Its fixed account passwords are for this disposable local VM; do not expose its provisioning WinRM endpoint to an untrusted network.

Shut down the guest completely, then create its baseline:

```sh
task vm:stop
task vm:snapshot
SETUP_EXE=/absolute/demo-app-setup.exe task test:e2e
```

The runner verifies `stopped` before every direct qcow2 snapshot operation, requests graceful shutdown and aborts if shutdown cannot be confirmed. It never force-kills a VM. Set `VM_DISK` explicitly when a bundle has multiple qcow2 disks; ambiguous disks are rejected. `VM_NAME`, `SNAPSHOT_NAME`, `VM_TEST_USER`, `UTMCTL` and timeout variables are overridable. Default UTM CLI lookup includes its app-bundle executable.

UTM guest-agent commands run as SYSTEM. `run-as-user.ps1` creates a temporary Interactive/Limited scheduled task for the logged-in standard account and propagates its output and exit code. The tests reject an administrator / SYSTEM token and disabled UAC or Defender. `utmctl exec` uses `--cmd`; file push and pull use stdin/stdout. A setup with `--silent --no-launch` is waited to completion and checked for a nonzero exit. Process replacement asserts the old PID, and uninstall waits for original / retired directories and its pending marker to disappear.

## CI and releases

CI uses the pinned Rust version and `--locked` for builds/tests. Linux performs Rust and shell checks, including optimized tests for the GLib backport; Windows continuously builds all shipping crates and the real native fixture. RustSec scanning fails on vulnerability advisories and unsoundness without advisory exclusions. RustSec skips local path dependencies, so a mandatory source gate verifies the reviewed fix for RUSTSEC-2024-0429 using its exact source hash, path override, version and lockfile provenance. Maintenance warnings are displayed for review; any future exception must name its advisory, dependency path, rationale and follow-up rather than silently ignoring a category.

Both tag pushes and manual releases resolve one commit SHA. Version validation, reusable CI and artifact compilation check out that same SHA. Release verification compares source and embedded stub hashes, package version, format version, ABI, execution manifest and absence of a dynamic MSVC runtime dependency. Native CI additionally checks the final packaged manifest / payload digest.

Release assets include the bundler, compatible setup stub, SHA256 files and `provenance.json`. Signing is opt-in: set repository variable `TWI_RELEASE_SIGN_PROGRAM` and secret `TWI_RELEASE_SIGN_ARGS` (a JSON argument array). Set `TWI_REQUIRE_RELEASE_SIGNATURE=true` to require valid timestamped Authenticode signatures before publication. Without that policy the provenance file records unsigned status explicitly. See [SIGNING.md](SIGNING.md).
