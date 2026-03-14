pub mod manifest;

pub use manifest::{InstallMetadata, SetupManifest};

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

/// Check if the current executable is a TWI-bundled setup
#[cfg(target_os = "windows")]
pub fn is_bundled() -> bool {
    libsui::find_section(TWI_RESOURCE)
        .ok()
        .flatten()
        .is_some()
}

/// Extract the setup manifest from the current executable
#[cfg(target_os = "windows")]
pub fn get_manifest() -> SetupManifest {
    let data = libsui::find_section(MANIFEST_RESOURCE)
        .expect("Failed to read manifest resource")
        .expect("Manifest resource not found");

    SetupManifest::from_binary(data).expect("Failed to deserialize manifest")
}

/// Extract the application data from the current executable
#[cfg(target_os = "windows")]
pub fn get_application_data() -> Vec<u8> {
    libsui::find_section(APPLICATION_RESOURCE)
        .expect("Failed to read application resource")
        .expect("Application resource not found")
        .to_vec()
}

/// Extract the bundle (tar archive) data from the current executable
#[cfg(target_os = "windows")]
pub fn get_bundle_data() -> Vec<u8> {
    libsui::find_section(BUNDLE_RESOURCE)
        .expect("Failed to read bundle resource")
        .expect("Bundle resource not found")
        .to_vec()
}

/// Extract the WebView2 installer data from the current executable, if bundled
#[cfg(target_os = "windows")]
pub fn get_webview2_data() -> Option<Vec<u8>> {
    libsui::find_section(WEBVIEW2_RESOURCE)
        .ok()
        .flatten()
        .map(|data| data.to_vec())
}

/// Get the WebView2 installer filename
#[cfg(target_os = "windows")]
pub fn get_webview2_filename() -> String {
    libsui::find_section(WEBVIEW2_RESOURCE_FILENAME)
        .ok()
        .flatten()
        .map(|data| String::from_utf8_lossy(data).to_string())
        .unwrap_or_else(|| "MicrosoftEdgeWebview2Setup.exe".to_string())
}
