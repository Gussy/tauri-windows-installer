use crate::Bundle;
use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::process::Command as Process;

pub struct Webview2 {
    pub bundled: bool,
    pub exe: String,
    pub data: Option<Vec<u8>>,
    pub installed: bool,
}

impl Bundle for Webview2 {
    fn load() -> Self {
        let bundled = env!("WEBVIEW2_BUNDLED") == "true";

        let mut data = None;
        if bundled {
            data = Some(Self::load_data());
        }

        Self {
            bundled,
            exe: env!("WEBVIEW2_BUNDLED_NAME").to_string(),
            data,
            installed: Self::is_installed(),
        }
    }

    fn load_data() -> Vec<u8> {
        include_bytes!(concat!(
            env!("OUT_DIR"),
            "\\",
            env!("WEBVIEW2_BUNDLED_NAME")
        ))
        .to_vec()
    }

    #[cfg(target_os = "windows")]
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

    #[cfg(target_os = "windows")]
    fn install(&self, quiet: bool) -> Result<()> {
        let args = if quiet {
            vec!["/silent", "/install"]
        } else {
            vec!["/install"]
        };

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

    #[cfg(not(target_os = "windows"))]
    fn is_installed() -> bool {
        true
    }

    #[cfg(not(target_os = "windows"))]
    fn install(&self, _quiet: bool) -> Result<()> {
        Err(anyhow!(
            "WebView2 installation is not supported on this platform."
        ))
    }
}
