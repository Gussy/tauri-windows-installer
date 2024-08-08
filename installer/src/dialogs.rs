use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO};

use crate::to_wide::ToWide;

pub fn show_overwrite_repair_dialog(name: &str, version: &str, silent: bool) -> bool {
    if silent {
        return true;
    }

    let lp_title = format!("{} Setup ({})", name, version).to_wide_null();
    let lp_text = format!("{} is already installed.\nThis application is installed on your computer. If it is not functioning correctly, you can attempt to repair it.\n\nDo you want to attempt to repair it?", name).to_wide_null();

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
