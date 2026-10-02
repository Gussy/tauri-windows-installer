#![forbid(unsafe_code)]

mod lock;
pub mod manifest;
mod validation;

#[cfg(feature = "bundler")]
pub mod bundle;

pub use lock::{acquire_install_lock, operation_lock_path, InstallLock};
pub use manifest::{
    InstallMetadata, SetupManifest, FORMAT_VERSION, MAX_METADATA_SIZE, STUB_ABI_MARKER,
};
pub use validation::*;

#[cfg(feature = "bundler")]
pub use bundle::{
    bundle, BundleError, BundleOptions, BundleOutput, SigningCommand, WebView2Embedding,
};

/// Resource name used as a marker to identify TWI-bundled executables
pub const TWI_RESOURCE: &str = "TWI_RESOURCE";

/// Resource name for the setup manifest
pub const MANIFEST_RESOURCE: &str = "TWI_MANIFEST";

/// Resource name for the application executable data
pub const APPLICATION_RESOURCE: &str = "TWI_APPLICATION";

/// Resource name for the tar bundle containing all application files
pub const BUNDLE_RESOURCE: &str = "TWI_BUNDLE";

/// Resource name for the WebView2 installer data
pub const WEBVIEW2_RESOURCE: &str = "TWI_WEBVIEW2";

/// Resource name for the WebView2 installer filename
pub const WEBVIEW2_RESOURCE_FILENAME: &str = "TWI_WEBVIEW2_FILENAME";

/// Filename for install metadata written to the install directory
pub const INSTALL_METADATA_FILENAME: &str = ".twi-meta.json";

/// Check if the current executable is a TWI-bundled setup.
#[cfg(target_os = "windows")]
pub fn is_bundled() -> bool {
    matches!(libsui::find_section(TWI_RESOURCE), Ok(Some(data)) if data == TWI_RESOURCE.as_bytes())
}

#[cfg(target_os = "windows")]
fn required_resource(name: &'static str) -> Result<&'static [u8], String> {
    libsui::find_section(name)
        .map_err(|e| format!("Cannot read {name}: {e}"))?
        .ok_or_else(|| format!("Required setup resource {name} is missing"))
}

#[cfg(target_os = "windows")]
pub fn get_manifest() -> Result<SetupManifest, String> {
    SetupManifest::from_binary(required_resource(MANIFEST_RESOURCE)?)
}

#[cfg(target_os = "windows")]
pub fn get_application_data() -> Result<&'static [u8], String> {
    required_resource(APPLICATION_RESOURCE)
}

/// Borrow the mapped resource instead of copying the full payload.
#[cfg(target_os = "windows")]
pub fn get_bundle_data() -> Result<&'static [u8], String> {
    required_resource(BUNDLE_RESOURCE)
}

#[cfg(target_os = "windows")]
pub fn get_webview2_data() -> Result<Option<&'static [u8]>, String> {
    libsui::find_section(WEBVIEW2_RESOURCE)
        .map_err(|e| format!("Cannot read WebView2 resource: {e}"))
}

#[cfg(target_os = "windows")]
pub fn get_webview2_filename() -> Result<String, String> {
    let filename = match libsui::find_section(WEBVIEW2_RESOURCE_FILENAME)
        .map_err(|e| format!("Cannot read WebView2 filename: {e}"))?
    {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|_| "Invalid UTF-8 WebView2 filename")?
            .to_owned(),
        None => "MicrosoftEdgeWebview2Setup.exe".into(),
    };
    validate_windows_filename(&filename)?;
    Ok(filename)
}
