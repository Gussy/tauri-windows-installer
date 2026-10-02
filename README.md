# Tauri Windows Installer

A Rust Windows installer for [Tauri](https://tauri.app/) apps. Setup installs per user, launches the app, and registers its uninstaller without a wizard. Applications live in `%LOCALAPPDATA%\Programs\<identifier>`. Application data beneath `%LOCALAPPDATA%\<identifier>` is preserved during upgrades and uninstall.

Inspired by [VeloPack](https://github.com/velopack/velopack). TWI does not provide an updater; use Tauri's updater integration separately.

## Build and package

Build the Windows stub with the pinned Rust toolchain and the Visual Studio C++ tools / Windows SDK. Build the installer before the bundler so the intended stub is embedded:

```powershell
cargo build --locked --release -p twi_installer
$env:TWI_SETUP_EXE = (Resolve-Path target\release\setup.exe).Path
cargo build --locked --release -p twi_bundler
.\target\release\bundler.exe --stub-info
.\target\release\bundler.exe -c path\to\tauri.conf.json -a path\to\app.exe
```

For a directory containing sidecars, specify its main executable. Nested relative paths are supported:

```powershell
.\target\release\bundler.exe -c tauri.conf.json -a dist\app --main-exe bin/app.exe
```

The bundler reads product name, version, identifier, publisher and icon settings from `tauri.conf.json`. It produces `{productName}-setup.exe`. Identifiers, paths and executable images are validated before packaging and again at installation.

For packaging on macOS or Linux, supply a real Windows stub built from compatible source. The development-only placeholder cannot produce an installer:

```sh
TWI_SETUP_EXE=/absolute/path/setup.exe cargo build --locked --release -p twi_bundler
./target/release/bundler -c tauri.conf.json -a app.exe
# A runtime override is also available:
./target/release/bundler --setup-exe /absolute/path/setup.exe -c tauri.conf.json -a app.exe
```

| CLI option | Purpose |
| --- | --- |
| `-c, --tauri-conf <PATH>` | Tauri configuration file |
| `-a, --app <PATH>` | Executable or directory to package |
| `-t, --title <TITLE>` | Display title; defaults to productName |
| `--main-exe <PATH>` | Relative main executable for a directory |
| `--setup-exe <PATH>` | Override the embedded Windows installer stub |
| `--stub-info` | Print stub SHA256, ABI/schema, version and manifest information |
| `-s, --sign-command <COMMAND>` | Sign the final packaged executable |
| `-o, --output-dir <DIR>` | Output directory; defaults to cwd |

Setup accepts `--silent` to suppress error dialogs and `--no-launch` to suppress the final application launch. Both are useful for automation. Errors still return a nonzero status and are logged.

## Library API

```rust
use twi_core::{bundle, BundleOptions};

let output = bundle(BundleOptions {
    setup_exe: std::fs::read("setup.exe")?,
    name: "my-app".into(),
    title: "My App".into(),
    version: "1.0.0".into(),
    identifier: "com.example.myapp".into(),
    publisher: "Example Inc.".into(),
    app: "dist/app".into(),
    main_exe: Some("bin/app.exe".into()),
    icon: None,
    webview2: None,
    sign_command: None,
    desktop_shortcut: true,
    output_dir: "dist/installers".into(),
    on_progress: Some(Box::new(|message| println!("{message}"))),
})?;
```

Enable the `bundler` feature on `twi_core`. Signing uses a program and argument vector; see [SIGNING.md](SIGNING.md).

## Uninstall integration

Call the library before normal app initialization:

```rust
fn main() {
    twi_uninstaller::handle_uninstall();
    // Normal app startup follows when no uninstall flag was present.
}
```

The installer registers `"app.exe" --uninstall`. The library finds and validates installation metadata, shares the setup operation lock, stops application processes and schedules cleanup. Nested main executables are supported. Failed cleanup retains a retry path; application data outside the managed binaries is preserved.

## Durability

The package contains a versioned JSON manifest, compressed tar payload, payload digest and unpacked size. The stub preserves its Windows `asInvoker` manifest and carries an explicit ABI marker. Setup validates and extracts into staging before replacing the live installation, keeps a recoverable transaction record and retains the old version until commit. Setup and uninstall share an exclusive per-application OS lock.

Desktop shortcuts are tracked as owned artifacts. An upgrade preserves a user's deletion and handles explicit title / shortcut setting changes. WebView2 Evergreen can be included through the plugin configuration; its bootstrapper needs internet access when installing a missing runtime. A package without a bootstrapper requires an already installed runtime.

The binaries target 64-bit Windows 10 and Windows 11. Packaging checks executable architecture; test the architecture and Windows versions you distribute.

## Development and validation

```sh
cargo test --locked
cargo clippy --locked -p twi_core --features bundler -p twi_bundler --all-targets -- -D warnings
bash tests/harness-regression.sh
```

Continuous CI also builds and tests the Windows installer/uninstaller and runs native failure / upgrade / cleanup checks. Local native validation is `task test:windows`. Standard-user VM validation is `task test:e2e`; its fixture retains UAC and Defender. See [ARCHITECTURE.md](ARCHITECTURE.md) for test setup, limitations and release checks, and [AUDIT.md](AUDIT.md) for the original audit evidence.

## Demo app

Install Node.js and pnpm, build the installer and bundler as above, then:

```sh
cd demo-app
pnpm install --frozen-lockfile
pnpm tauri build -- --locked
cd ..
./target/release/bundler -c demo-app/src-tauri/tauri.conf.json -a target/release/demo-app.exe
```

The demo build requires Windows for its application executable. Its output is `demo-app-setup.exe`.

## License

MIT.
