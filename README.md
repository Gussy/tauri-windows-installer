# Tauri Windows Installer

An [MVP](https://en.wikipedia.org/wiki/Minimum_viable_product) for a simple and modern "one click" windows installer for [Tauri apps](https://tauri.app/).

This work is heavily inspired by [VeloPack](https://github.com/velopack/velopack) and uses many of the same concepts, however unlike VeloPack this work only handles the installation and uninstallation of Tauri applications, only on Windows, and has no support for any update mechanisims.

## Goals

- Simple installer - _Small file size and code complexity_
- Opinionated implementation - _Lack of features is the main feature_
- One click installs - _No wizards, just install and launch the app immedietly_

While this implementation is standalone and could be used with any Tauri projects, the end goal is to have this work (or something based on it) merged into the Tauri core as a built-in option for bundling on windows.

### Compatibility

Tauri apps on Windows require WebView2 which is [included with Windows 10](https://learn.microsoft.com/en-us/microsoft-365-apps/deploy/webview2-install#webview2-runtime-installation) _20H2_ and later versions.

Windows 10 versions earlier than _20H2_, the [WebView2 Evergreen Bootstrapper](https://developer.microsoft.com/en-us/microsoft-edge/webview2/?form=MA13LH#download) (~1.6MB) can be bundled in to the setup executable. If WebView2 is not detected, if present the bootstrapper will be run, to streamline downloading and installation of WebView2 as part of the installation process. The offline installers (~155MB) could be included in the future if required.

Windows 8 and earlier may work, but are not explicitly supported right now.

## Components

### Bundler `bundler.exe`

```text
Tauri Windows Installer Bundler

Usage: bundler.exe --tauri-conf <TAURI_CONF> --app <APP>

Options:
  -t, --tauri-conf <TAURI_CONF>  Path to the Tauri configuration file
  -a, --app <APP>                Path to application to bundle
  -h, --help                     Print help
  -V, --version                  Print version
```

The bundler is used to construct a custom setup executable for installing the Tauri application on the host system.

The base `setup.exe` file is included in the bundler with the rust `include_bytes!()`. The bundler then uses that built in binary as a base to append a setup manifest, webview2 installer (if required) and the application.

### Installer

The installer crate builds both a skeleton setup application (`setup.exe`) along with a library `tauri_windows_installer`:

- The setup application is what's used by the bundler as a base for the final setup executable.
- The library is used by the target Tauri app to add a `--uninstall` hook to the application, to handle uninstalling the app.

#### Installation overview

1. The bundled package is extracted, this contains a setup manifest and all the bundled files
1. If WebView2 is not installed **and** the boostrapper is included, the boostrapper executable will be written to disk and spawned
1. The installation directory is determined, defaulting to `%APPDATA%\{app-identifier}`
1. If the installation directory doesn't exist, it's created, if it does exist and the target application executable exists inside, a dialog is shown prompting to overwrite or cancel the installation
    1. When overwriting, the existing installation directory is moved to a temporary location
1. The installation directory is emptied
1. The application is installed. This copies the application to the installation directory and spawns it as a detached process
1. If the previous step failed **and** an existing installation is being overwritten, a rollback occurs by renaming the temporary installation back to it's original name. The setup process then exits
1. An uninstallation entry is written to the the `HKEY_CURRENT_USER` registry, using `{productName}.exe --uninstall` as the uninstallation command

### Uninstaller

The uninstaller is built into the main Tauri application, by calling a function from the `tauri_windows_installer` library. This adds a `--uninstall` argument handler to the Tauri application

```rust
tauri_windows_installer::handle_uninstall(&"{app_id}");
```

#### Uninstallation overview

1. _TODO: Kill all running application processes_
1. _TODO: Remove the installation directory_
1. Remove the entry from the `HKEY_CURRENT_USER` registry

## TODO

- Bundler
  - Add `-s, --setup-version` arguments to print the currently built-in `setup.exe` version
  - Add an icon to the packaged `{productName}-setup.exe`
  - Add other resource information like name, version, date etc to the `{productName}-setup.exe`
  - Get a human friendly application title from somewhere (cli argument?)
- Installer
  - Embed versioning into `setup.exe`
  - Check the OS version and architecture
  - Improve the required space calculation
- Other
  - Setup GitHub Actions to build and release
