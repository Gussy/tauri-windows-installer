use anyhow::Result;
use windows::{
    core::{GUID, PWSTR},
    Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
};

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
