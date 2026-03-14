# Tauri Windows Installer

A pure-Rust, zero-click Windows installer for [Tauri](https://tauri.app/) apps. Click the setup executable and the app installs to `%LOCALAPPDATA%`, launches immediately, and registers an uninstaller. No wizard, no options.

Inspired by [VeloPack](https://github.com/velopack/velopack). Unlike VeloPack, this is Tauri-specific and has no update mechanism (Tauri has its own updater plugin).

## How it works

The bundler embeds your built app into a setup executable as PE resources (via [libsui](https://github.com/nicolo-ribaudo/libsui)). The setup executable extracts, installs, and launches — all in one click.

```
core/            Shared library — types, constants, and bundling API
bundler/         CLI that creates setup executables
installer/       The setup.exe stub (runs on end-user machines)
uninstaller/     Library linked into your app for --uninstall handling
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for details on the PE resource approach, bundle format, and programmatic API.

## Usage

### CLI

```sh
# Build the installer stub (Windows) and bundler
cargo build --release

# Bundle a Tauri app into a setup executable
bundler -c path/to/tauri.conf.json -a path/to/app.exe
```

The bundler reads `tauri.conf.json` for product name, version, identifier, publisher, and icons. Output: `{name}-setup.exe`.

```
Options:
  -c, --tauri-conf <PATH>     Path to tauri.conf.json
  -a, --app <PATH>            Application executable or directory
  -t, --title <TITLE>         App title (defaults to productName from config)
      --main-exe <NAME>       Main exe name (required when --app is a directory)
  -s, --sign-command <CMD>    Code signing command (file path appended as last arg)
  -o, --output-dir <DIR>      Output directory (defaults to current directory)
```

### As a library

```rust
use twi_core::{bundle, BundleOptions};

let output = bundle(BundleOptions {
    setup_exe: std::fs::read("setup.exe")?,
    name: "my-app".into(),
    title: "My App".into(),
    version: "1.0.0".into(),
    identifier: "com.example.myapp".into(),
    publisher: "Example Inc.".into(),
    app: "target/release/my-app.exe".into(),
    main_exe: None,
    icon: None,
    webview2: None,
    sign_command: None,
    output_dir: "dist".into(),
    on_progress: Some(Box::new(|msg| println!("{msg}"))),
})?;
```

Add to `Cargo.toml`: `twi_core = { version = "0.1", features = ["bundler"] }`

### Uninstall integration

Link the uninstaller library into your Tauri app:

```rust
fn main() {
    #[cfg(target_os = "windows")]
    if twi_uninstaller::handle_uninstall() {
        std::process::exit(0);
    }
    // ... normal app startup
}
```

The installer registers `"app.exe" --uninstall` in the Windows registry. When triggered, it kills running processes, removes files, cleans up the registry, and deletes itself.

## Features

- Single-file setup executable (app + metadata + optional WebView2 bundled as PE resources)
- Zstd-compressed tar bundle (supports single exe or full directory with sidecars)
- PE version info (shows version/publisher in Windows file properties)
- Code signing support (`--sign-command` or `signCommand` in tauri.conf.json)
- WebView2 Evergreen bootstrapper bundling (auto-installs if not present)
- Silent overwrite on reinstall with rollback on failure
- Per-user install to `%LOCALAPPDATA%` (no admin required)

## Compatibility

| | 64-bit | 32-bit |
|---|---|---|
| Windows 11 | Supported | N/A |
| Windows 10 | Supported | Not supported |

WebView2 is included with Windows 10 20H2+. For earlier versions, the Evergreen bootstrapper (~1.6 MB) can be bundled.

## Tauri integration

**Short-term**: Use `beforeBundleCommand` in `tauri.conf.json` to run the bundler CLI after `tauri build`.

**Long-term**: Call `twi_core::bundle()` directly from the Tauri bundler. The library has no Tauri dependency.

## Development

Develop on macOS, test on Windows. The workspace default members (`core` + `bundler`) compile on macOS. The `installer` and `uninstaller` are Windows-only.

```sh
cargo check          # Check core + bundler
cargo test           # Run all tests
cargo check -p twi_core --features bundler   # Check bundling API
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full development guide including VM-based e2e testing.

## License

MIT
