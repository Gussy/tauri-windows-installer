use std::path::PathBuf;

use ::windows::core::PCWSTR;
use anyhow::{anyhow, Result};
use chrono::prelude::*;
use twi_core::SetupManifest;
use windows::{
    core::{GUID, Interface, PWSTR},
    Win32::Storage::FileSystem::GetDiskFreeSpaceExW,
    Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, IPersistFile,
    },
    Win32::UI::Shell::{IShellLinkW, ShellLink, FOLDERID_Desktop, FOLDERID_LocalAppData, SHGetKnownFolderPath},
};
use winreg::enums::*;
use winreg::RegKey;

const UNINSTALL_STR: &'static str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

pub fn get_local_app_data() -> Result<String> {
    get_known_folder(&FOLDERID_LocalAppData)
}

fn get_known_folder(folder_id: *const GUID) -> Result<String> {
    // SAFETY: SHGetKnownFolderPath is a well-defined Win32 API. The folder_id pointer
    // comes from a Windows SDK constant (FOLDERID_*). The returned PWSTR is valid
    // until we convert it to a Rust String.
    unsafe {
        let flag = windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG(0);
        let result =
            SHGetKnownFolderPath(folder_id, flag, None).expect("Failed to get known folder path");
        pwstr_to_string(result)
    }
}

fn pwstr_to_string(input: PWSTR) -> Result<String> {
    // SAFETY: The PWSTR comes from a successful Win32 API call that guarantees
    // a valid null-terminated UTF-16 string. to_hstring reads until the null terminator.
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

    let uninstall_cmd = format!("\"{}\" --uninstall", main_exe_path);

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
    app_key.set_value("Publisher", &manifest.publisher)?;
    app_key.set_value("UninstallString", &uninstall_cmd)?;
    app_key.set_value("QuietUninstallString", &uninstall_cmd)?;
    app_key.set_value("EstimatedSize", &(folder_size as u32 / 1024))?;
    app_key.set_value("NoModify", &1u32)?;
    app_key.set_value("NoRepair", &1u32)?;
    app_key.set_value("Language", &0x0409u32)?;

    Ok(())
}

pub fn create_desktop_shortcut(manifest: &SetupManifest, root_path: &PathBuf) -> Result<()> {
    println!("Creating desktop shortcut...");

    let desktop_path = get_known_folder(&FOLDERID_Desktop)?;
    let shortcut_path = std::path::Path::new(&desktop_path).join(format!("{}.lnk", manifest.title));
    let target_path = root_path.join(&manifest.application);
    let target_str: Vec<u16> = string_to_u16(target_path.to_string_lossy().as_ref());
    let working_dir: Vec<u16> = string_to_u16(root_path.to_string_lossy().as_ref());
    let description: Vec<u16> = string_to_u16(&manifest.title);
    let shortcut_path_wide: Vec<u16> = string_to_u16(shortcut_path.to_string_lossy().as_ref());

    // SAFETY: COM calls are well-defined Win32 APIs. All PCWSTR values are valid
    // null-terminated UTF-16 strings created by string_to_u16. CoInitializeEx is
    // called before any COM object creation, and CoUninitialize is called after.
    unsafe {
        // COM may already be initialized (e.g. by WebView2 detection). CoInitializeEx
        // returns S_FALSE if already initialized on this thread, or RPC_E_CHANGED_MODE
        // if initialized with a different concurrency model — both are acceptable since
        // we just need COM available, not to own its lifetime.
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let we_initialized_com = hr.is_ok();
        if hr.is_err() {
            // RPC_E_CHANGED_MODE means COM is already initialized with a different
            // threading model — we can still use it for shell link creation.
            const RPC_E_CHANGED_MODE: i32 = 0x80010106_u32 as i32;
            if hr.0 != RPC_E_CHANGED_MODE {
                hr.ok()?;
            }
        }

        let shell_link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;

        shell_link.SetPath(PCWSTR(target_str.as_ptr()))?;
        shell_link.SetWorkingDirectory(PCWSTR(working_dir.as_ptr()))?;
        shell_link.SetDescription(PCWSTR(description.as_ptr()))?;
        shell_link.SetIconLocation(PCWSTR(target_str.as_ptr()), 0)?;

        let persist_file: IPersistFile = shell_link.cast()?;
        persist_file.Save(PCWSTR(shortcut_path_wide.as_ptr()), true)?;

        if we_initialized_com {
            CoUninitialize();
        }
    }

    println!("Desktop shortcut created at: {}", shortcut_path.display());
    Ok(())
}

/// Gets the free disk space for the provided path.
pub fn get_free_space(root_path_str: &str) -> Result<u64> {
    let mut free_space: u64 = 0;
    let root_pcwstr = string_to_u16(root_path_str);
    let root_pcwstr: PCWSTR = PCWSTR(root_pcwstr.as_ptr());

    // SAFETY: root_pcwstr is a valid null-terminated UTF-16 string created from
    // string_to_u16. free_space is a valid mutable reference to a stack-allocated u64.
    let result = unsafe { GetDiskFreeSpaceExW(root_pcwstr, None, None, Some(&mut free_space)) };

    if result.is_err() {
        return Err(anyhow!(
            "Failed to retrieve free disk space for path: {}",
            root_path_str
        ));
    }

    Ok(free_space)
}
