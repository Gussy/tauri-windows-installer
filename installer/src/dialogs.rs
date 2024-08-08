use bundler::SetupManifest;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO};

pub fn show_overwrite_repair_dialog(manifest: &SetupManifest, silent: bool) -> bool {
    if silent {
        return true;
    }

    let lp_title = format!("{} Setup ({})", manifest.name, manifest.version)
        .as_str()
        .to_wide_null();
    let lp_text = format!("{} is already installed.\nThis application is installed on your computer. If it is not functioning correctly, you can attempt to repair it.\n\nDo you want to attempt to repair it?", manifest.name)
        .as_str()
        .to_wide_null();

    let result = unsafe {
        MessageBoxW(
            HWND(std::ptr::null_mut()),
            PCWSTR(lp_text.as_ptr()),
            PCWSTR(lp_title.as_ptr()),
            MB_YESNO | MB_ICONQUESTION,
        )
    };

    return result == IDYES;
}

trait ToWide {
    fn _to_wide(&self) -> Vec<u16>;
    fn to_wide_null(&self) -> Vec<u16>;
}
impl<T> ToWide for T
where
    T: AsRef<OsStr>,
{
    fn _to_wide(&self) -> Vec<u16> {
        self.as_ref().encode_wide().collect()
    }
    fn to_wide_null(&self) -> Vec<u16> {
        self.as_ref().encode_wide().chain(Some(0)).collect()
    }
}
