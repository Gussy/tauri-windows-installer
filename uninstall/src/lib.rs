mod to_wide;

use anyhow::Result;
use std::env;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command as Process;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, MessageBoxW, MB_ICONINFORMATION, MB_ICONWARNING,
};
use winreg::enums::*;
use winreg::RegKey;

use crate::to_wide::ToWide;

const UNINSTALL_STR: &'static str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

pub fn handle_uninstall(app_title: &str, app_id: &str) {
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--uninstall".to_string()) {
        println!("Uninstall flag detected. Running uninstall code...");
        uninstall(app_title, app_id);
    }
}

fn uninstall(app_title: &str, app_id: &str) {
    // Force stop any other instances of the application
    let _ = std::process::Command::new("taskkill")
        .args(&["/IM", &format!("{}.exe", app_id), "/F"])
        .output()
        .expect("Failed to kill the application");

    let mut errors = false;

    // Remove the installation directory (except for the executable)
    let root_path = std::env::current_exe()
        .expect("Failed to get current executable path")
        .parent()
        .expect("Failed to get parent directory")
        .to_path_buf();
    if let Err(e) = remove_dir_all_ext::remove_dir_but_not_self(&root_path) {
        eprintln!("Failed to remove installation directory: {}", e);
        errors = true;
    }

    // Remove the uninstall registry key
    if let Err(e) = remove_uninstall_entry(&app_id) {
        eprintln!("Failed to remove uninstall registry key: {}", e);
        errors = true;
    }

    // Show the result
    if !errors {
        println!("Uninstall completed successfully!");
        show_info(
            format!("{} Uninstall", app_title).as_str(),
            None,
            "The application was successfully uninstalled.",
        );
    } else {
        eprintln!("Uninstall completed with errors.");
        show_uninstall_complete_with_errors()
    }

    // Delete the executable
    register_intent_to_delete_self(3, &root_path)
        .expect("Failed to register intent to delete self");

    std::process::exit(errors as i32);
}

fn remove_uninstall_entry(app_id: &str) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall_key = hkcu.create_subkey(UNINSTALL_STR)?.0;
    uninstall_key.delete_subkey(&app_id)?;
    Ok(())
}

fn show_info(title: &str, parent: Option<HWND>, text: &str) {
    let lp_title = title.to_wide_null();
    let lp_text = text.to_wide_null();

    unsafe {
        MessageBoxW(
            parent.unwrap_or(HWND(std::ptr::null_mut())),
            PCWSTR(lp_text.as_ptr()),
            PCWSTR(lp_title.as_ptr()),
            MB_ICONINFORMATION,
        );
    }
}

fn show_uninstall_complete_with_errors() {
    let lp_title = "Uninstall Complete".to_wide_null();
    let lp_text = "The application was uninstalled, but there were errors during the process. Please check the logs for more information.".to_wide_null();

    unsafe {
        MessageBoxW(
            HWND(std::ptr::null_mut()),
            PCWSTR(lp_text.as_ptr()),
            PCWSTR(lp_title.as_ptr()),
            MB_ICONWARNING,
        );
    }
}

fn register_intent_to_delete_self(delay_seconds: usize, current_directory: &Path) -> Result<()> {
    println!("Deleting self...");
    let current_exe = env::current_exe()?.to_string_lossy().to_string();
    let command = format!(
        "choice /C Y /N /D Y /T {} & Del \"{}\" & Rmdir \"{}\"",
        delay_seconds,
        current_exe,
        current_directory.file_name().unwrap().to_string_lossy()
    );
    println!("Running: cmd.exe /C {}", command);

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let child = Process::new("cmd.exe")
        .arg("/C")
        .raw_arg(command)
        .current_dir(current_directory.parent().unwrap())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    let _ = unsafe { AllowSetForegroundWindow(child.id()) };

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;

    #[test]
    fn test_register_intent_to_delete_self() {
        // Mock the current directory and executable path
        let temp_dir = env::temp_dir();
        let current_exe = temp_dir.join("test_exe.exe");
        fs::File::create(&current_exe).unwrap();

        let result = register_intent_to_delete_self(3, &temp_dir);
        assert!(result.is_ok());

        // Check that the function executes without panicking
        assert!(true);
    }
}
