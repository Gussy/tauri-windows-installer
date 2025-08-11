pub mod manifest;

pub use crate::manifest::SetupManifest;

use libsui::find_section;

// Resource names must be null-terminated strings
pub const TWI_RESOURCE: &str = "__TWI";
pub const MANIFEST_RESOURCE: &str = "__TWI_MANIFEST";
pub const WEBVIEW_RESOURCE: &str = "__TWI_WEBVIEW";
pub const WEBVIEW_RESOURCE_FILENAME: &str = "__TWI_WEBVIEW_EXE";
pub const APPLICATION_RESOURCE: &str = "__TWI_APPLICATION";

pub fn get_manifest() -> SetupManifest {
    let data = find_section(MANIFEST_RESOURCE).expect("Failed to find manifest section");
    bincode::deserialize(&data).expect("Failed to deserialize manifest")
}

pub fn get_application_data() -> Vec<u8> {
    find_section(APPLICATION_RESOURCE)
        .map(|data| data.to_vec())
        .expect("Failed to find application section")
}

pub fn get_webview2_data() -> Option<Vec<u8>> {
    Some(
        find_section(WEBVIEW_RESOURCE)
            .map(|data| data.to_vec())
            .expect("Failed to find application section"),
    )
}

pub fn get_webview2_filename() -> String {
    // Attempt to find the section and convert the &[u8] to a &str
    find_section(WEBVIEW_RESOURCE_FILENAME)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or("")
        .to_string()
}

pub fn is_bundled() -> bool {
    find_section(TWI_RESOURCE).is_some()
}
