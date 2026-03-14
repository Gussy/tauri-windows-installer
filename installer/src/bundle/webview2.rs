use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::process::Command as Process;

pub(crate) struct WebView2 {
    pub bundled: bool,
    pub exe: String,
    pub data: Option<Vec<u8>>,
    pub installed: bool,
}

impl WebView2 {
    pub fn load() -> Self {
        let data = twi_core::get_webview2_data();
        let bundled = data.is_some();
        let exe = twi_core::get_webview2_filename();
        let installed = Self::is_installed();

        Self {
            bundled,
            exe,
            data,
            installed,
        }
    }

    fn is_installed() -> bool {
        use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
        use windows::core::{PCWSTR, PWSTR};
        let mut versioninfo = PWSTR::null();
        let result = unsafe {
            GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut versioninfo)
        };

        if result.is_err() || versioninfo == PWSTR::null() {
            return false;
        }

        let version = take_pwstr(versioninfo);
        if version.len() > 0 {
            println!("WebView2 version: {}", version);
            return true;
        }

        false
    }

    pub fn install(&self) -> Result<()> {
        let args = vec!["/silent", "/install"];

        // Copy the installer to a temp location
        let temp_dir = env::temp_dir();
        let installer_path = temp_dir.join(self.exe.clone());
        fs::write(&installer_path, self.data.as_ref().unwrap()).expect("Failed to write installer");

        // Run the installer
        println!("Running installer: '{:?}', args={:?}", installer_path, args);
        let mut cmd = Process::new(installer_path).args(&args).spawn()?;
        let result: i32 = cmd
            .wait()?
            .code()
            .ok_or_else(|| anyhow!("Unable to get installer exit code."))?;

        match result {
            0 => Ok(()),           // success
            -2147219416 => Ok(()), // already installed
            _ => Err(anyhow!("Installer failed with exit code: {}", result)),
        }
    }
}
