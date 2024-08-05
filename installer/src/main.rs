mod application;
mod bundle;
mod dialogs;
mod manifest;
mod process;
mod webview2;
mod windows;

use crate::application::Application;
use crate::bundle::Bundle;
use crate::manifest::extract_manifest_from_data;
use crate::process::find_and_kill_processes_from_directory;
use crate::webview2::Webview2;
use crate::windows::{get_local_app_data, string_to_u16};

use ::windows::core::PCWSTR;
use ::windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use anyhow::{Context, Result};
use process::spawn_detached_process;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{fs, io};

fn main() {
    // Handle bundled application
    let app = Application::load();
    println!("Application: {}", app.name);

    // Handle bundled WebView2 runtime
    let webview2 = Webview2::load();
    println!("Webview2 bundled: {}", webview2.bundled);

    // Check if WebView2 runtime is installed
    let webview2_installed = webview2.installed;
    println!("Webview2 installed: {}", webview2_installed);
    if !webview2_installed {
        println!("Installing webview2 runtime...");
        webview2
            .install(false)
            .expect("Failed to install webview2 runtime");
    }

    // Determine the installation directory
    println!("Determining install directory...");
    let appdata = get_local_app_data().expect("Failed to get local app data path");
    let root_path = Path::new(&appdata).join(&app.id);
    if !root_path.exists() {
        fs::create_dir_all(&root_path).expect("Failed to create installation directory");
    }
    let root_path_str = root_path.to_str().unwrap();
    println!("Installation Directory: {:?}", root_path_str);

    // Check if there is enough space to install the application
    let required_space = app.size as u64;
    println!("Required disk space: {}", required_space);

    let mut free_space: u64 = 0;
    let root_pcwstr = string_to_u16(root_path_str);
    let root_pcwstr: PCWSTR = PCWSTR(root_pcwstr.as_ptr());
    if let Ok(()) = unsafe { GetDiskFreeSpaceExW(root_pcwstr, None, None, Some(&mut free_space)) } {
        if free_space < required_space {
            panic!(
                "{} requires at least {} disk space to be installed. There is only {} available.",
                app.id,
                format_bytes(required_space),
                format_bytes(free_space)
            );
        }
    }

    println!(
        "There is {} free space available at destination, this package requires {}.",
        format_bytes(free_space),
        format_bytes(required_space)
    );

    // TODO: Check if the application supports this OS version and architecture
    let binary_data = app.data.clone();
    match extract_manifest_from_data(&binary_data) {
        Ok(_) => println!("Manifest extracted successfully."),
        Err(e) => eprintln!("Failed to extract manifest: {}", e),
    }

    let mut root_path_renamed = String::new();

    // Check if the application is already installed
    if !is_directory_empty(&root_path).unwrap() {
        let result = dialogs::show_overwrite_repair_dialog(&app, false);

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
    remove_dir_all::ensure_empty_dir(&root_path).expect("Failed to clean installation directory");

    // Install the application
    let install_result = install(&app, &root_path);

    // Handle rollback if installation fails
    if install_result.is_ok() {
        println!("Installation completed successfully!");
        if !root_path_renamed.is_empty() {
            println!("Removing rollback directory...");
            let _ = fs::remove_dir_all(&root_path_renamed);
        }
    } else {
        println!("Installation failed! {}", install_result.unwrap_err());
        if !root_path_renamed.is_empty() {
            println!("Rolling back installation...");
            let _ = find_and_kill_processes_from_directory(root_path_str);
            let _ = fs::remove_dir_all(&root_path);
            let _ = fs::rename(&root_path_renamed, &root_path);
        }
    }

    // TODO:
    // - Set the windows registry keys
    // - Handle uninstall with command line flag
}

fn install(app: &Application, root_path: &PathBuf) -> Result<()> {
    println!("Starting installation!");

    println!("Extracting application to installation directory...");
    let application_path = root_path.join(&app.exe);

    // Create and write to the application file
    let mut file = fs::File::create(&application_path).with_context(|| {
        format!(
            "Failed to create application file at {:?}",
            application_path
        )
    })?;
    file.write_all(&app.data)
        .with_context(|| format!("Failed to write application data to {:?}", application_path))?;

    // Close the file
    drop(file);

    // Start the application
    spawn_detached_process(application_path.clone())
        .with_context(|| format!("Failed to start application at {:?}", application_path))?;

    Ok(())
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
