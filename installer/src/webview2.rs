use std::env;
use std::fs;

#[cfg(target_os = "windows")]
use winreg::RegKey;

pub struct Webview2 {
    pub bundled: bool,
    pub data: Option<Vec<u8>>,
    pub installed: bool,
}

impl Webview2 {
    pub fn load() -> Self {
        let bundled = env!("WEBVIEW2_BUNDLED") == "true";

        let mut data = None;
        if bundled {
            let name = env!("WEBVIEW2_BUNDLED_NAME").to_string();
            data = Some(Self::load_data(&name));
        }

        Self {
            bundled,
            data: data,
            installed: Self::installed(),
        }
    }

    fn load_data(name: &str) -> Vec<u8> {
        let out_dir = env!("OUT_DIR");
        let binary_path = format!("{}/{}", out_dir, name);
        println!("Webview2 exe path: {}", binary_path);
        fs::read(&binary_path).expect("Failed to read external program binary")
    }

    #[cfg(target_os = "windows")]
    fn installed() -> bool {
        let system_wide_64bit = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}").is_err();
        let system_wide_32bit = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(
                "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            )
            .is_err();
        let user_32bit_64bit = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(
                "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
            )
            .is_err();
        if system_wide_64bit && system_wide_32bit && user_32bit_64bit {
            return true;
        }

        false
    }

    #[cfg(not(target_os = "windows"))]
    fn installed() -> bool {
        true
    }
}
