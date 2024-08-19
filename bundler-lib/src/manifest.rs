use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct SetupManifest {
    pub name: String,
    pub title: String,
    pub version: String,
    pub identifier: String,
    pub application: String,
}

impl SetupManifest {
    /// Deserialize from binary data, returning a `Result` to handle errors gracefully
    pub fn from_binary(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }

    /// Serialize to binary data, returning a `Result` to handle errors gracefully
    pub fn to_binary(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization_deserialization() {
        // Create a SetupManifest instance
        let manifest = SetupManifest {
            name: "TestApp".to_string(),
            title: "Test Application Installer".to_string(),
            version: "1.0.0".to_string(),
            identifier: "com.example.testapp".to_string(),
            application: "testapp.exe".to_string(),
        };

        // Serialize the manifest to binary
        let serialized = manifest.to_binary();
        assert!(serialized.is_ok(), "Serialization failed");

        let serialized = serialized.unwrap();

        // Deserialize the binary back into a SetupManifest instance
        let deserialized = SetupManifest::from_binary(&serialized);
        assert!(deserialized.is_ok(), "Deserialization failed");

        let deserialized_manifest = deserialized.unwrap();

        // Check if the deserialized manifest matches the original
        assert_eq!(manifest.name, deserialized_manifest.name);
        assert_eq!(manifest.title, deserialized_manifest.title);
        assert_eq!(manifest.version, deserialized_manifest.version);
        assert_eq!(manifest.identifier, deserialized_manifest.identifier);
        assert_eq!(manifest.application, deserialized_manifest.application);
    }

    #[test]
    fn test_deserialization_error() {
        // Create an invalid binary data (empty or corrupted)
        let invalid_data: Vec<u8> = vec![];

        // Attempt to deserialize it into a SetupManifest
        let result = SetupManifest::from_binary(&invalid_data);

        // It should fail, resulting in an Err
        assert!(
            result.is_err(),
            "Deserialization should fail with invalid data"
        );
    }

    #[test]
    fn test_empty_fields_serialization() {
        // Create a SetupManifest instance with empty fields
        let manifest = SetupManifest {
            name: "".to_string(),
            title: "".to_string(),
            version: "".to_string(),
            identifier: "".to_string(),
            application: "".to_string(),
        };

        // Serialize the manifest to binary
        let serialized = manifest.to_binary();
        assert!(serialized.is_ok(), "Serialization of empty fields failed");

        let serialized = serialized.unwrap();

        // Deserialize the binary back into a SetupManifest instance
        let deserialized = SetupManifest::from_binary(&serialized);
        assert!(
            deserialized.is_ok(),
            "Deserialization of empty fields failed"
        );

        let deserialized_manifest = deserialized.unwrap();

        // Check if the deserialized manifest matches the original
        assert_eq!(manifest.name, deserialized_manifest.name);
        assert_eq!(manifest.title, deserialized_manifest.title);
        assert_eq!(manifest.version, deserialized_manifest.version);
        assert_eq!(manifest.identifier, deserialized_manifest.identifier);
        assert_eq!(manifest.application, deserialized_manifest.application);
    }
}
