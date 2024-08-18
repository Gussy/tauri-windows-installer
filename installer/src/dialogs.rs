use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_ICONQUESTION, MB_OK, MB_YESNO,
};

use crate::to_wide::ToWide;

pub fn show_overwrite_repair_dialog(title: &str, name: &str, version: &str, silent: bool) -> bool {
    if silent {
        return true;
    }

    let lp_title = format!("{} Setup ({})", title, version).to_wide_null();
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

pub fn show_error_dialog(title: &str, message: &str) {
    // Convert the title and message to wide strings
    let lp_title = title.to_wide_null();
    let lp_message = message.to_wide_null();

    unsafe {
        MessageBoxW(
            HWND(std::ptr::null_mut()),
            PCWSTR(lp_message.as_ptr()),
            PCWSTR(lp_title.as_ptr()),
            MB_OK | MB_ICONERROR,
        )
    };
}
