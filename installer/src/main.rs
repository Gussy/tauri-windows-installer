// Prevent additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bundle;
mod dialogs;
mod process;
mod to_wide;
mod windows;

use crate::bundle::Bundle;
use crate::bundle::{Application, WebView2};
use crate::dialogs::{show_error_dialog, show_overwrite_repair_dialog};
use crate::process::find_and_kill_processes_from_directory;
use crate::windows::{get_free_space, get_local_app_data};

use bundler::extract_package;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::path::{Path, PathBuf};
use std::{fs, io};

fn main() {
    // Get name of the currently running bniary at runtime
    let binary_name = PathBuf::from(
        std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
    );

    // Extract packages
    let package = extract_package(&binary_name);
    let manifest = &package.manifest;
    println!("Application: {}", manifest.name);

    // Handle bundled application
    let app = Application::load(&package);
    println!(
        "Application size: {}",
        format_bytes(app.data.len().try_into().unwrap())
    );

    // Handle bundled WebView2 runtime
    let webview2 = WebView2::load(&package);
    println!("Webview2 bundled: {}", webview2.bundled);

    // Check if WebView2 runtime is installed
    let webview2_installed = webview2.installed;
    println!("Webview2 installed: {}", webview2_installed);
    if !webview2_installed {
        println!("Installing webview2 runtime...");
        webview2
            .install(false, &PathBuf::new())
            .expect("Failed to install webview2 runtime");
    }

    // Determine the installation directory
    println!("Determining install directory...");
    let appdata = get_local_app_data().expect("Failed to get local app data path");
    let root_path = Path::new(&appdata).join(&manifest.identifier);
    if !root_path.exists() {
        fs::create_dir_all(&root_path).expect("Failed to create installation directory");
    }
    let root_path_str = root_path.to_str().unwrap();
    println!("Installation Directory: {:?}", root_path_str);

    // Check if there is enough space to install the application
    let required_space = app.size as u64;
    println!("Required disk space: {}", format_bytes(required_space));

    // Check if there is enough space to install the application
    match get_free_space(root_path_str) {
        Ok(free_space) => {
            if free_space < required_space {
                show_error_dialog(
                    "Not enough disk space",
                    &format!(
                        "{} requires at least {} disk space to be installed. There is only {} available.",
                        manifest.title,
                        format_bytes(required_space),
                        format_bytes(free_space)
                    ),
                );
                return;
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

    // Check if the application is already installed
    if !is_directory_empty(&root_path).unwrap() {
        let result =
            show_overwrite_repair_dialog(&manifest.title, &manifest.name, &manifest.version, false);

        if !result {
            println!("User cancelled installation");
            return;
        }
        println!("User chose to overwrite existing installation.");

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
            .expect("Failed to rename existing installation directory");
    }

    println!("Preparing and cleaning installation directory...");
    remove_dir_all_ext::ensure_empty_dir(&root_path)
        .expect("Failed to clean installation directory");

    // Install the application
    let quiet = false;
    let install_result = app.install(quiet, &root_path);

    // Handle rollback if installation fails
    if install_result.is_ok() == false {
        println!("Installation failed! {}", install_result.unwrap_err());
        if !root_path_renamed.is_empty() {
            println!("Rolling back installation...");
            let _ = find_and_kill_processes_from_directory(root_path_str);
            let _ = fs::remove_dir_all(&root_path);
            let _ = fs::rename(&root_path_renamed, &root_path);
        }

        // exit the installer with an error code
        std::process::exit(1);
    }

    println!("Installation completed successfully!");
    if !root_path_renamed.is_empty() {
        println!("Removing rollback directory...");
        let _ = fs::remove_dir_all(&root_path_renamed);
    }

    // Write the uninstall registry keys
    windows::write_uninstall_entry(&manifest, &root_path)
        .expect("Failed to write uninstall registry key");
}

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

fn is_directory_empty(path: &Path) -> io::Result<bool> {
    let mut entries = fs::read_dir(path)?;
    Ok(entries.next().is_none())
}

fn generate_random_string(length: usize) -> String {
    let rng = thread_rng();
    rng.sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}
