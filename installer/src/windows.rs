use std::path::PathBuf;

use ::windows::core::PCWSTR;
use anyhow::{anyhow, Result};
use bundler::SetupManifest;
use chrono::prelude::*;
use windows::{
    core::{GUID, PWSTR},
    Win32::Storage::FileSystem::GetDiskFreeSpaceExW,
    Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
};
use winreg::enums::*;
use winreg::RegKey;

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

fn string_to_u16<P: AsRef<str>>(input: P) -> Vec<u16> {
    let input = input.as_ref();
    input.encode_utf16().chain(Some(0)).collect::<Vec<u16>>()
}

pub fn write_uninstall_entry(manifest: &SetupManifest, root_path: &PathBuf) -> Result<()> {
    println!("Writing uninstall registry key...");
    let root_path_str = root_path.to_string_lossy().to_string();
    let main_exe_path_binding = root_path.join(&manifest.application);
    let main_exe_path = main_exe_path_binding.to_str().unwrap();

    let folder_size = fs_extra::dir::get_size(&root_path).unwrap();
    let version_str = &manifest.version;

    let now = Local::now();
    let formatted_date = format!("{}{:02}{:02}", now.year(), now.month(), now.day());

    let uninstall_cmd = format!("{} --uninstall", &main_exe_path);

    // Open or create the app-specific subkey
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall_key = hkcu.create_subkey(UNINSTALL_STR)?.0;
    let app_key = uninstall_key.create_subkey(&manifest.identifier)?.0;

    // Set the values for the app-specific subkey
    app_key.set_value("DisplayIcon", &main_exe_path)?;
    app_key.set_value("DisplayName", &manifest.name)?;
    app_key.set_value("DisplayVersion", version_str)?;
    app_key.set_value("InstallDate", &formatted_date)?;
    app_key.set_value("InstallLocation", &root_path_str)?;
    app_key.set_value("Publisher", &"")?; // TODO: Set publisher
    app_key.set_value("UninstallString", &uninstall_cmd)?;
    app_key.set_value("EstimatedSize", &(folder_size as u32 / 1024))?;
    app_key.set_value("NoModify", &1u32)?;
    app_key.set_value("NoRepair", &1u32)?;
    app_key.set_value("Language", &0x0409u32)?;

    Ok(())
}

/// Gets the free disk space for the provided path.
pub fn get_free_space(root_path_str: &str) -> Result<u64> {
    let mut free_space: u64 = 0;
    let root_pcwstr = string_to_u16(root_path_str);
    let root_pcwstr: PCWSTR = PCWSTR(root_pcwstr.as_ptr());

    let result = unsafe { GetDiskFreeSpaceExW(root_pcwstr, None, None, Some(&mut free_space)) };

    if result.is_err() {
        return Err(anyhow!(
            "Failed to retrieve free disk space for path: {}",
            root_path_str
        ));
    }

    Ok(free_space)
}
