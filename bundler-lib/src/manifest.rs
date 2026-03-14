use serde::{Deserialize, Serialize};

/// Manifest describing the setup package contents
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SetupManifest {
    pub name: String,
    pub title: String,
    pub version: String,
    pub identifier: String,
    pub application: String,
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
        };

        let binary = manifest.to_binary().unwrap();
        let result = SetupManifest::from_binary(&binary).unwrap();

        assert_eq!(result.name, "MyApp");
        assert_eq!(result.title, "My App");
        assert_eq!(result.version, "0.1.0");
        assert_eq!(result.identifier, "com.example.myapp");
        assert_eq!(result.application, "myapp.exe");
    }

    #[test]
    fn test_manifest_empty_strings() {
        let manifest = SetupManifest {
            name: String::new(),
            title: String::new(),
            version: String::new(),
            identifier: String::new(),
            application: String::new(),
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
}
