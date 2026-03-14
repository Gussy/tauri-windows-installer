//! Setup manifest and install metadata types.
//!
//! These types are shared between the bundler (which writes them) and the
//! installer/uninstaller (which reads them).

use serde::{Deserialize, Serialize};

/// Manifest describing the setup package contents.
///
/// Serialized to bincode and embedded as a PE resource in the setup executable.
/// The installer reads this at runtime to determine product name, version,
/// install paths, etc.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SetupManifest {
    /// Product name used for directory names and filenames.
    pub name: String,
    /// Human-readable application title shown in the installer UI.
    pub title: String,
    /// Semantic version string (e.g. `"1.2.3"`).
    pub version: String,
    /// Reverse-domain identifier (e.g. `"com.example.myapp"`).
    pub identifier: String,
    /// Main executable filename (e.g. `"my-app.exe"`).
    pub application: String,
    /// Publisher name shown in Windows "Add/Remove Programs".
    pub publisher: String,
}

impl SetupManifest {
    /// Serialize the manifest to binary using bincode
    pub fn to_binary(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize a manifest from binary data
    pub fn from_binary(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

/// Metadata written to disk during installation, read by the uninstaller.
///
/// Stored as JSON at [`INSTALL_METADATA_FILENAME`](crate::INSTALL_METADATA_FILENAME)
/// in the application's install directory.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct InstallMetadata {
    /// Human-readable application title.
    pub app_title: String,
    /// Reverse-domain application identifier.
    pub app_id: String,
    /// Main executable filename.
    pub app_exe: String,
    /// Installed version string.
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_roundtrip() {
        let manifest = SetupManifest {
            name: "TestApp".to_string(),
            title: "Test Application".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.example.testapp".to_string(),
            application: "testapp.exe".to_string(),
            publisher: "Example Inc.".to_string(),
        };

        let binary = manifest.to_binary().expect("Failed to serialize manifest");
        let deserialized =
            SetupManifest::from_binary(&binary).expect("Failed to deserialize manifest");

        assert_eq!(manifest, deserialized);
    }

    #[test]
    fn test_manifest_fields() {
        let manifest = SetupManifest {
            name: "MyApp".to_string(),
            title: "My App".to_string(),
            version: "0.1.0".to_string(),
            identifier: "com.example.myapp".to_string(),
            application: "myapp.exe".to_string(),
            publisher: "My Publisher".to_string(),
        };

        let binary = manifest.to_binary().unwrap();
        let result = SetupManifest::from_binary(&binary).unwrap();

        assert_eq!(result.name, "MyApp");
        assert_eq!(result.title, "My App");
        assert_eq!(result.version, "0.1.0");
        assert_eq!(result.identifier, "com.example.myapp");
        assert_eq!(result.application, "myapp.exe");
        assert_eq!(result.publisher, "My Publisher");
    }

    #[test]
    fn test_manifest_empty_strings() {
        let manifest = SetupManifest {
            name: String::new(),
            title: String::new(),
            version: String::new(),
            identifier: String::new(),
            application: String::new(),
            publisher: String::new(),
        };

        let binary = manifest.to_binary().unwrap();
        let result = SetupManifest::from_binary(&binary).unwrap();
        assert_eq!(manifest, result);
    }

    #[test]
    fn test_manifest_invalid_binary() {
        let result = SetupManifest::from_binary(&[0xFF, 0xFF, 0xFF]);
        assert!(result.is_err());
    }

    #[test]
    fn test_install_metadata_roundtrip() {
        let metadata = InstallMetadata {
            app_title: "My App".to_string(),
            app_id: "com.example.myapp".to_string(),
            app_exe: "myapp.exe".to_string(),
            version: "1.0.0".to_string(),
        };

        let json = serde_json::to_string_pretty(&metadata).unwrap();
        let deserialized: InstallMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(metadata, deserialized);
    }
}
