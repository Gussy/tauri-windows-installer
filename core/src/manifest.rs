//! Versioned package manifests and installation ownership metadata.

use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;
pub const STUB_ABI_MARKER: &[u8] = b"TWI_SETUP_ABI_V1\0";
const MANIFEST_MAGIC: &[u8] = b"TWI-MANIFEST\n";
pub const MAX_METADATA_SIZE: usize = 64 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SetupManifest {
    pub name: String,
    pub title: String,
    pub version: String,
    pub identifier: String,
    /// Relative executable path, with forward slashes for nested layouts.
    pub application: String,
    pub publisher: String,
    pub desktop_shortcut: bool,
    /// Total regular file bytes before compression. Zero denotes a legacy package.
    #[serde(default)]
    pub unpacked_size: u64,
    /// SHA-256 of the compressed bundle, in lowercase hexadecimal.
    #[serde(default)]
    pub bundle_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestEnvelope {
    format_version: u32,
    manifest: SetupManifest,
}

impl SetupManifest {
    /// Serialize a bounded JSON envelope with an explicit schema version.
    pub fn to_binary(&self) -> Result<Vec<u8>, String> {
        crate::validate_manifest(self)?;
        let envelope = ManifestEnvelope {
            format_version: FORMAT_VERSION,
            manifest: self.clone(),
        };
        let mut bytes = MANIFEST_MAGIC.to_vec();
        bytes.extend(serde_json::to_vec(&envelope).map_err(|e| e.to_string())?);
        if bytes.len() > MAX_METADATA_SIZE {
            return Err("Package manifest exceeds the metadata size limit".into());
        }
        Ok(bytes)
    }

    /// Read the current format and the exact fixed-width legacy bincode layout.
    /// The legacy reader avoids retaining an unmaintained serialization dependency.
    pub fn from_binary(data: &[u8]) -> Result<Self, String> {
        if data.len() > MAX_METADATA_SIZE {
            return Err("Package manifest exceeds the metadata size limit".into());
        }
        let manifest = if let Some(json) = data.strip_prefix(MANIFEST_MAGIC) {
            let envelope: ManifestEnvelope = serde_json::from_slice(json)
                .map_err(|e| format!("Invalid package manifest: {e}"))?;
            if envelope.format_version != FORMAT_VERSION {
                return Err(format!(
                    "Unsupported package format {} (supported: {})",
                    envelope.format_version, FORMAT_VERSION
                ));
            }
            envelope.manifest
        } else {
            decode_legacy(data)?
        };
        crate::validate_manifest(&manifest)?;
        Ok(manifest)
    }
}

fn decode_legacy(mut data: &[u8]) -> Result<SetupManifest, String> {
    fn string(data: &mut &[u8]) -> Result<String, String> {
        let length_bytes: [u8; 8] = data
            .get(..8)
            .ok_or("Truncated legacy manifest")?
            .try_into()
            .map_err(|_| "Invalid legacy string length")?;
        *data = &data[8..];
        let length = usize::try_from(u64::from_le_bytes(length_bytes))
            .map_err(|_| "Legacy string length is too large")?;
        if length > MAX_METADATA_SIZE {
            return Err("Legacy string exceeds the metadata size limit".into());
        }
        let bytes = data
            .get(..length)
            .ok_or("Truncated legacy manifest string")?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| "Legacy manifest contains invalid UTF-8")?
            .to_owned();
        *data = &data[length..];
        Ok(value)
    }
    Ok(SetupManifest {
        name: string(&mut data)?,
        title: string(&mut data)?,
        version: string(&mut data)?,
        identifier: string(&mut data)?,
        application: string(&mut data)?,
        publisher: string(&mut data)?,
        desktop_shortcut: match data {
            [0] => false,
            [1] => true,
            _ => return Err("Invalid or trailing legacy manifest data".into()),
        },
        unpacked_size: 0,
        bundle_sha256: String::new(),
    })
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct InstallMetadata {
    /// Zero is the original JSON metadata; one is the current ownership schema.
    #[serde(default)]
    pub format_version: u32,
    pub app_title: String,
    pub app_id: String,
    pub app_exe: String,
    pub version: String,
    pub desktop_shortcut: bool,
    /// The owned shortcut's title, which can differ from the current product title.
    #[serde(default)]
    pub shortcut_title: Option<String>,
}

impl InstallMetadata {
    pub fn validate(&self) -> Result<(), String> {
        if self.format_version > FORMAT_VERSION {
            return Err(format!(
                "Unsupported install metadata version {}",
                self.format_version
            ));
        }
        crate::validate_identifier(&self.app_id)?;
        crate::validate_relative_executable(&self.app_exe)?;
        crate::validate_windows_filename(&self.app_title)?;
        if let Some(title) = &self.shortcut_title {
            crate::validate_windows_filename(title)?;
        }
        semver::Version::parse(&self.version)
            .map_err(|e| format!("Invalid installed version: {e}"))?;
        Ok(())
    }

    pub fn owned_shortcut_title(&self) -> Option<&str> {
        if !self.desktop_shortcut {
            return None;
        }
        self.shortcut_title.as_deref().or(Some(&self.app_title))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> SetupManifest {
        SetupManifest {
            name: "TestApp".into(),
            title: "Test Application".into(),
            version: "1.2.3".into(),
            identifier: "com.example.testapp".into(),
            application: "bin/testapp.exe".into(),
            publisher: "Example Inc.".into(),
            desktop_shortcut: true,
            unpacked_size: 1024,
            bundle_sha256: "ab".repeat(32),
        }
    }

    #[test]
    fn versioned_manifest_roundtrip() {
        let manifest = manifest();
        let bytes = manifest.to_binary().unwrap();
        assert!(bytes.starts_with(MANIFEST_MAGIC));
        assert_eq!(SetupManifest::from_binary(&bytes).unwrap(), manifest);
    }

    #[test]
    fn legacy_fixture_remains_readable() {
        let expected = manifest();
        // Layout emitted by bincode 1.3.3's serialize function: u64 lengths + UTF-8 + bool.
        let mut bytes = Vec::new();
        for value in [
            &expected.name,
            &expected.title,
            &expected.version,
            &expected.identifier,
            &expected.application,
            &expected.publisher,
        ] {
            bytes.extend((value.len() as u64).to_le_bytes());
            bytes.extend(value.as_bytes());
        }
        bytes.push(1);
        let mut legacy = expected;
        legacy.unpacked_size = 0;
        legacy.bundle_sha256.clear();
        assert_eq!(SetupManifest::from_binary(&bytes).unwrap(), legacy);
        bytes.push(0);
        assert!(SetupManifest::from_binary(&bytes).is_err());
    }

    #[test]
    fn unsupported_versions_and_oversized_lengths_are_errors() {
        let mut bytes = MANIFEST_MAGIC.to_vec();
        bytes.extend(
            serde_json::to_vec(&serde_json::json!({
                "format_version": 999, "manifest": manifest()
            }))
            .unwrap(),
        );
        assert!(SetupManifest::from_binary(&bytes)
            .unwrap_err()
            .contains("Unsupported"));
        assert!(SetupManifest::from_binary(&u64::MAX.to_le_bytes()).is_err());
        assert!(SetupManifest::from_binary(&vec![0; MAX_METADATA_SIZE + 1]).is_err());
    }

    #[test]
    fn unsafe_legacy_identifiers_are_rejected() {
        let mut invalid = manifest();
        invalid.identifier.clear();
        assert!(invalid.to_binary().is_err());
    }

    #[test]
    fn old_metadata_preserves_owned_shortcut() {
        let metadata: InstallMetadata = serde_json::from_str(
            r#"{
            "app_title":"Old title", "app_id":"com.example.testapp", "app_exe":"app.exe",
            "version":"1.0.0", "desktop_shortcut":true
        }"#,
        )
        .unwrap();
        metadata.validate().unwrap();
        assert_eq!(metadata.owned_shortcut_title(), Some("Old title"));
        assert_eq!(metadata.format_version, 0);
    }
}
