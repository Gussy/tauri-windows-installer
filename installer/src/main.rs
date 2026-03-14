// Prevent additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod bundle;
#[cfg(target_os = "windows")]
mod process;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This binary is Windows-only");
    std::process::exit(1);
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(e) = run_installer() {
        eprintln!("Installation failed: {}", e);
        std::process::exit(1);
    }
}

#[cfg(target_os = "windows")]
fn run_installer() -> Result<(), String> {
    use crate::process::find_and_kill_processes_from_directory;
    use crate::windows::{get_free_space, get_local_app_data};

    use std::fs;
    use std::path::Path;

    // OS version check: require Windows 10+ (build >= 10240)
    check_os_version();

    // Check if this is a TWI-bundled executable
    if !bundler::is_bundled() {
        return Err("This executable is not a valid TWI setup package.".to_string());
    }

    // Extract manifest
    let manifest = bundler::get_manifest();
    println!("Application: {}", manifest.name);

    // Load bundle data
    let bundle_data = bundler::get_bundle_data();
    let bundle_size = bundle_data.len() as u64;
    println!("Bundle size: {}", format_bytes(bundle_size));

    // Handle bundled WebView2 runtime
    let webview2 = bundle::WebView2::load();
    println!("Webview2 bundled: {}", webview2.bundled);
    println!("Webview2 installed: {}", webview2.installed);
    if !webview2.installed {
        println!("Installing webview2 runtime...");
        webview2
            .install()
            .map_err(|e| format!("Failed to install webview2 runtime: {}", e))?;
    }

    // Determine the installation directory
    println!("Determining install directory...");
    let appdata = get_local_app_data()
        .map_err(|e| format!("Failed to get local app data path: {}", e))?;
    let root_path = Path::new(&appdata).join(&manifest.identifier);
    if !root_path.exists() {
        fs::create_dir_all(&root_path)
            .map_err(|e| format!("Failed to create installation directory: {}", e))?;
    }
    let root_path_str = root_path.to_str().unwrap();
    println!("Installation Directory: {:?}", root_path_str);

    // Check if there is enough space to install the application
    let required_space = bundle_size;
    println!("Required disk space: {}", format_bytes(required_space));

    match get_free_space(root_path_str) {
        Ok(free_space) => {
            if free_space < required_space {
                return Err(format!(
                    "{} requires at least {} disk space to be installed. There is only {} available.",
                    manifest.title,
                    format_bytes(required_space),
                    format_bytes(free_space)
                ));
            } else {
                println!(
                    "There is {} free space available at destination, this package requires {}.",
                    format_bytes(free_space),
                    format_bytes(required_space)
                );
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }

    let mut root_path_renamed = String::new();

    // Check if the application is already installed — always overwrite silently
    if !is_directory_empty(&root_path).unwrap() {
        println!("Existing installation found, overwriting...");

        // Force stop the application if it is running
        match find_and_kill_processes_from_directory(root_path_str) {
            Ok(_) => println!("All processes from {} have been terminated.", root_path_str),
            Err(e) => eprintln!("Failed to terminate processes: {}", e),
        }

        // Rename the existing installation directory
        root_path_renamed = format!("{}_{}", root_path_str, generate_random_string(8));
        println!(
            "Renaming existing directory to '{}' to allow rollback...",
            root_path_renamed
        );
        fs::rename(&root_path, &root_path_renamed)
            .map_err(|e| format!("Failed to rename existing installation directory: {}", e))?;
    }

    println!("Preparing and cleaning installation directory...");
    remove_dir_all_ext::ensure_empty_dir(&root_path)
        .map_err(|e| format!("Failed to clean installation directory: {}", e))?;

    // Extract bundle (tar archive) to install directory
    let install_result = extract_bundle(&bundle_data, &root_path);

    // Handle rollback if installation fails
    if let Err(e) = install_result {
        println!("Installation failed! {}", e);
        if !root_path_renamed.is_empty() {
            println!("Rolling back installation...");
            let _ = find_and_kill_processes_from_directory(root_path_str);
            let _ = fs::remove_dir_all(&root_path);
            let _ = fs::rename(&root_path_renamed, &root_path);
        }

        return Err(format!("Installation failed: {}", e));
    }

    println!("Installation completed successfully!");
    if !root_path_renamed.is_empty() {
        println!("Removing rollback directory...");
        let _ = fs::remove_dir_all(&root_path_renamed);
    }

    // Write the uninstall registry keys
    windows::write_uninstall_entry(&manifest, &root_path)
        .map_err(|e| format!("Failed to write uninstall registry key: {}", e))?;

    // Write install metadata for the uninstaller
    let metadata = bundler::InstallMetadata {
        app_title: manifest.title.clone(),
        app_id: manifest.identifier.clone(),
        app_exe: manifest.application.clone(),
        version: manifest.version.clone(),
    };
    let meta_path = root_path.join(bundler::INSTALL_METADATA_FILENAME);
    fs::write(
        &meta_path,
        serde_json::to_string_pretty(&metadata).map_err(|e| format!("Failed to serialize metadata: {}", e))?,
    )
    .map_err(|e| format!("Failed to write install metadata: {}", e))?;

    // Launch the application
    let app_path = root_path.join(&manifest.application);
    process::spawn_detached_process(app_path)
        .map_err(|e| format!("Failed to start application: {}", e))?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn extract_bundle(compressed_data: &[u8], install_dir: &std::path::Path) -> Result<(), String> {
    use std::io::{Cursor, Read};

    println!("Decompressing bundle...");
    let mut tar_data = Vec::new();
    ruzstd::streaming_decoder::StreamingDecoder::new(Cursor::new(compressed_data))
        .map_err(|e| format!("Failed to initialize decompressor: {}", e))?
        .read_to_end(&mut tar_data)
        .map_err(|e| format!("Failed to decompress bundle: {}", e))?;

    println!("Extracting bundle to installation directory...");
    let mut archive = tar::Archive::new(Cursor::new(tar_data));
    archive
        .unpack(install_dir)
        .map_err(|e| format!("Failed to extract bundle: {}", e))?;
    println!("Bundle extracted successfully.");
    Ok(())
}

#[cfg(target_os = "windows")]
fn check_os_version() {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(nt_key) = hklm.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion") {
        let build: String = nt_key
            .get_value("CurrentBuildNumber")
            .unwrap_or_else(|_| "0".to_string());
        if let Ok(build_num) = build.parse::<u32>() {
            if build_num < 10240 {
                eprintln!(
                    "Warning: This installer requires Windows 10 or later (build >= 10240, found {}).",
                    build_num
                );
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < units.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.2} {}", size, units[unit_index])
}

#[cfg(target_os = "windows")]
fn is_directory_empty(path: &std::path::Path) -> std::io::Result<bool> {
    let mut entries = std::fs::read_dir(path)?;
    Ok(entries.next().is_none())
}

#[cfg(target_os = "windows")]
fn generate_random_string(length: usize) -> String {
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};
    let rng = thread_rng();
    rng.sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}
