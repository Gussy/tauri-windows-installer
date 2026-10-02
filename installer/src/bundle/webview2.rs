use anyhow::{anyhow, bail, Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) struct WebView2 {
    data: Option<&'static [u8]>,
    filename: String,
}

impl WebView2 {
    pub fn load() -> Result<Self> {
        Ok(Self {
            data: twi_core::get_webview2_data().map_err(anyhow::Error::msg)?,
            filename: twi_core::get_webview2_filename().map_err(anyhow::Error::msg)?,
        })
    }

    fn is_installed() -> bool {
        use webview2_com::{take_pwstr, Microsoft::Web::WebView2::Win32::*};
        use windows::core::{PCWSTR, PWSTR};
        let mut version = PWSTR::null();
        let result =
            unsafe { GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut version) };
        if version.is_null() {
            return false;
        }
        let version = take_pwstr(version); // Frees any returned buffer even on failure.
        result.is_ok() && !version.is_empty()
    }

    pub fn ensure_installed(&self) -> Result<()> {
        if Self::is_installed() {
            return Ok(());
        }
        let data = self.data.ok_or_else(|| anyhow!(
            "Microsoft WebView2 Runtime is required but is not installed or included in this package. Install WebView2 Runtime, then run setup again."
        ))?;
        let temp = tempfile::Builder::new().prefix("twi-webview2-").tempdir()?;
        // Use a fixed basename inside an exclusive random directory. The embedded
        // filename is informational and can never redirect a write outside it.
        let installer = temp.path().join("webview2-setup.exe");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&installer)
            .with_context(|| {
                format!("Cannot write bundled WebView2 installer {}", self.filename)
            })?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        verify_microsoft_signature(&installer)?;
        let mut child = Command::new(&installer)
            .args(["/silent", "/install"])
            .current_dir(temp.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .context("Cannot start the bundled WebView2 installer")?;
        let deadline = Instant::now() + Duration::from_secs(300);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let stopped = child.kill();
                if stopped.is_ok() {
                    let _ = child.wait();
                }
                bail!("WebView2 installation timed out after five minutes. Check network access and retry setup.");
            }
            thread::sleep(Duration::from_millis(250));
        };
        let code = status
            .code()
            .ok_or_else(|| anyhow!("The WebView2 installer returned no exit code."))?;
        if code != 0 && code != -2147219416 {
            bail!("WebView2 installation failed (exit code {code}). Check network access and retry setup.");
        }
        // Exit status alone is insufficient when Edge Update is policy-disabled
        // or another runtime installation is incomplete.
        if !Self::is_installed() {
            bail!("WebView2 installation returned success, but the runtime is unavailable. Repair WebView2 Runtime and retry setup.");
        }
        Ok(())
    }
}

fn verify_microsoft_signature(path: &std::path::Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Security::Cryptography::{
        CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    };
    use windows::Win32::Security::WinTrust::*;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut information = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        ..Default::default()
    };
    let mut trust = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
        ..Default::default()
    };
    trust.Anonymous.pFile = &mut information;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // WINTRUST_DATA and its pointed-to file information remain alive until the
    // verification state has been closed, including certificate inspection.
    let verification = unsafe {
        WinVerifyTrust(
            HWND(-1isize as *mut _),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    let verified = (|| -> Result<()> {
        if verification != 0 {
            bail!("The bundled WebView2 installer does not have a trusted Authenticode signature ({verification:#x}).");
        }
        let provider = unsafe { WTHelperProvDataFromStateData(trust.hWVTStateData) };
        if provider.is_null() {
            bail!("Cannot inspect the WebView2 signature provider.");
        }
        let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, false, 0) };
        if signer.is_null() {
            bail!("The WebView2 installer has no certificate signer.");
        }
        let signer = unsafe { &*signer };
        if signer.csCertChain == 0 || signer.pasCertChain.is_null() {
            bail!("The WebView2 signer certificate chain is unavailable.");
        }
        let certificate = unsafe { (*signer.pasCertChain).pCert };
        if certificate.is_null() {
            bail!("The WebView2 signer certificate is unavailable.");
        }
        let length = unsafe {
            CertGetNameStringW(certificate, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, None)
        };
        if length == 0 || length > 4096 {
            bail!("Cannot read the WebView2 signer identity.");
        }
        let mut name = vec![0u16; length as usize];
        let read = unsafe {
            CertGetNameStringW(
                certificate,
                CERT_NAME_SIMPLE_DISPLAY_TYPE,
                0,
                None,
                Some(&mut name),
            )
        };
        if read != length {
            bail!("Cannot read the WebView2 signer identity.");
        }
        let name = String::from_utf16(&name[..name.len().saturating_sub(1)])?;
        if name != "Microsoft Corporation" {
            bail!("The bundled WebView2 installer is signed by an unexpected publisher: {name}");
        }
        Ok(())
    })();
    trust.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            HWND(-1isize as *mut _),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    verified
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsigned_webview_installer_is_rejected_without_running_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("webview2-setup.exe");
        std::fs::write(&path, crate::executable::fixture_image()).unwrap();
        assert!(super::verify_microsoft_signature(&path).is_err());
    }
}
