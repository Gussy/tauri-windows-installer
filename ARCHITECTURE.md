# Architecture

This document describes the architecture of `tauri-windows-installer` (TWI) for Tauri maintainers and contributors.

## Crate Overview

```
bundler-lib/     Core library shared by all crates
bundler/         CLI tool that creates setup executables
installer/       Windows setup executable (runs on end-user machines)
uninstall/       Uninstall library (linked into the installed app)
```

### bundler-lib (`twi_bundler_lib`)

Shared library with two roles:

1. **Data types** — `SetupManifest`, `InstallMetadata`, PE resource constants. Used by all crates.
2. **Bundling API** (behind `bundler` cargo feature) — `bundle()` function, `BundleOptions`, `BundleError`. Used by the CLI and available for programmatic integration.

The `bundler` feature gates `tar`, `zstd`, `editpe`, `indexmap`, and `shell-words` dependencies. Without the feature, only `libsui`, `serde`, `bincode`, and `serde_json` are pulled in — this is important because `zstd` has C bindings that cannot cross-compile from macOS to Windows.

### bundler (CLI)

Thin CLI wrapper around `bundler::bundle()`. Responsibilities:
- Parse Tauri configuration (`tauri.conf.json`)
- Resolve icon, WebView2, and sign command from config
- Load the embedded `setup.exe` stub via `include_bytes!`
- Map CLI args + Tauri config → `BundleOptions`

### installer (`twi_installer`)

The setup executable that runs on Windows. Built as `setup.exe` and embedded into the bundler at compile time. At runtime it:
1. Extracts `SetupManifest` and bundle data from its own PE resources
2. Decompresses the zstd-compressed tar archive
3. Installs to `%LOCALAPPDATA%/{identifier}`
4. Optionally installs WebView2
5. Writes `InstallMetadata` for the uninstaller
6. Creates Windows registry uninstall entry

### uninstall (`twi_uninstall`)

Library linked into the installed application. When the app is launched with `--uninstall`, it reads `InstallMetadata`, removes files, cleans up registry entries, and schedules self-deletion.

## How Bundling Works

The setup executable is a standard Windows PE with application data embedded as named PE resources (via [libsui](https://github.com/nicolo-ribaudo/libsui)):

| Resource Name | Content |
|---|---|
| `TWI_RESOURCE` | Marker — identifies the PE as a TWI installer |
| `TWI_MANIFEST` | `SetupManifest` serialized with bincode |
| `TWI_BUNDLE` | zstd-compressed tar archive of the application |
| `TWI_WEBVIEW2` | WebView2 bootstrapper executable (optional) |
| `TWI_WEBVIEW2_FILENAME` | WebView2 installer filename |

The bundler also writes PE version info (FileVersion, ProductName, etc.) via `editpe`.

## Bundle Format

The application bundle is a tar archive compressed with zstd level 19:

- **Single file**: tar containing just the `.exe`
- **Directory**: tar containing the full directory tree (e.g., app exe + DLLs + assets)

When bundling a directory, `--main-exe` specifies which executable to launch after installation.

## Programmatic Usage

```rust
use bundler::{bundle, BundleOptions};
use std::path::PathBuf;

let options = BundleOptions {
    setup_exe: std::fs::read("path/to/setup.exe")?,
    name: "my-app".into(),
    title: "My App".into(),
    version: "1.0.0".into(),
    identifier: "com.example.myapp".into(),
    publisher: "Example Inc.".into(),
    app: PathBuf::from("target/release/my-app.exe"),
    main_exe: None,
    icon: None,
    webview2: None,
    sign_command: None,
    output_dir: PathBuf::from("dist"),
    on_progress: Some(Box::new(|msg| println!("{msg}"))),
};

let output = bundle(options)?;
// output.path = "dist/my-app-setup.exe"
// output.size = file size in bytes
```

The caller provides the `setup.exe` bytes — how the stub is sourced is a deployment concern (embedded via `include_bytes!`, downloaded, etc.).

## Cross-Compilation

The workspace is designed so that `installer` and `uninstall` can be cross-compiled from macOS to `x86_64-pc-windows-msvc`:

- `bundler-lib` without the `bundler` feature has no C dependencies
- `installer` uses `ruzstd` (pure Rust) instead of `zstd` (C bindings)
- The `bundler` crate (which needs `zstd` for compression) only runs on the build machine

## Tauri Integration

**Short-term**: Use `beforeBundleCommand` in `tauri.conf.json` to run the bundler CLI after the Tauri build.

**Long-term**: Call `bundler::bundle()` directly from the Tauri bundler. The library has no Tauri dependency — it accepts plain Rust types.

The sign command resolution chain is: CLI flag → plugin config (`signCommand`) → `tauri.conf.json` (`bundle.windows.signCommand`).
