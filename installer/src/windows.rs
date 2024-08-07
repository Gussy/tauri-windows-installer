use std::path::PathBuf;

use anyhow::Result;
use chrono::prelude::*;
use windows::{
    core::{GUID, PWSTR},
    Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
};
use winreg::enums::*;
use winreg::RegKey;

use crate::application::Application;

const UNINSTALL_STR: &'static str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

pub fn get_local_app_data() -> Result<String> {
    get_known_folder(&FOLDERID_LocalAppData)
}

fn get_known_folder(folder_id: *const GUID) -> Result<String> {
    unsafe {
        let flag = windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG(0);
        let result =
            SHGetKnownFolderPath(folder_id, flag, None).expect("Failed to get known folder path");
        pwstr_to_string(result)
    }
}

fn pwstr_to_string(input: PWSTR) -> Result<String> {
    unsafe {
        let hstring = input.to_hstring()?;
        let string = hstring.to_string_lossy();
        Ok(string.trim_end_matches('\0').to_string())
    }
}

pub fn string_to_u16<P: AsRef<str>>(input: P) -> Vec<u16> {
    let input = input.as_ref();
    input.encode_utf16().chain(Some(0)).collect::<Vec<u16>>()
}

pub fn write_uninstall_entry(app: &Application, root_path: &PathBuf) -> Result<()> {
    println!("Writing uninstall registry key...");
    let root_path_str = root_path.to_string_lossy().to_string();
    let main_exe_path_binding = root_path.join(&app.exe);
    let main_exe_path = main_exe_path_binding.to_str().unwrap();

    let folder_size = fs_extra::dir::get_size(&root_path).unwrap();
    let version_str = &app.version;

    let now = Local::now();
    let formatted_date = format!("{}{:02}{:02}", now.year(), now.month(), now.day());

    let uninstall_cmd = format!("\"{}\" --uninstall", &main_exe_path);
    let uninstall_quiet = format!("\"{}\" --uninstall --silent", &main_exe_path);

    // Open or create the app-specific subkey
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall_key = hkcu.create_subkey(UNINSTALL_STR)?.0;
    let app_key = uninstall_key.create_subkey(&app.id)?.0;

    // Set the values for the app-specific subkey
    app_key.set_value("DisplayIcon", &main_exe_path)?;
    app_key.set_value("DisplayName", &app.name)?;
    app_key.set_value("DisplayVersion", version_str)?;
    app_key.set_value("InstallDate", &formatted_date)?;
    app_key.set_value("InstallLocation", &root_path_str)?;
    app_key.set_value("Publisher", &"")?; // TODO: Set publisher
    app_key.set_value("QuietUninstallString", &uninstall_quiet)?;
    app_key.set_value("UninstallString", &uninstall_cmd)?;
    app_key.set_value("EstimatedSize", &(folder_size / 1024))?;
    app_key.set_value("NoModify", &1u32)?;
    app_key.set_value("NoRepair", &1u32)?;
    app_key.set_value("Language", &0x0409u32)?;

    Ok(())
}
