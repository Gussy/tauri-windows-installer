pub mod exe_packager;
pub mod metadata;
pub mod plugin_config;
pub mod webview2;

pub use crate::exe_packager::{ExePackager, SetupManifest};
pub use crate::metadata::MetadataEntry;

use metadata::METADATA_OFFSET_SIZE;
use serde_json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use webview2::WEBVIEW2_EVERGREEN_EXE;

/// A package containing files and a manifest
#[derive(Debug)]
pub struct SetupPackage {
    pub files: HashMap<String, Vec<u8>>,
    pub manifest: SetupManifest,
}

/// Extracts the manifest and files from a packaged executable
pub fn extract_package(exe_path: &Path) -> SetupPackage {
    let mut file = File::open(exe_path).expect("Failed to open executable file");

    // Read the offset of the metadata at the end of the file
    file.seek(SeekFrom::End(-(METADATA_OFFSET_SIZE as i64)))
        .expect("Failed to seek to metadata offset");
    let mut offset_buf = [0; METADATA_OFFSET_SIZE];
    file.read_exact(&mut offset_buf)
        .expect("Failed to read metadata offset");
    let metadata_offset: u64 = String::from_utf8_lossy(&offset_buf)
        .trim()
        .parse()
        .expect("Failed to parse metadata offset");

    // Calculate the length of the metadata
    let metadata_length = file.metadata().expect("Failed to get file metadata").len()
        - metadata_offset
        - METADATA_OFFSET_SIZE as u64;

    // Read the metadata
    file.seek(SeekFrom::Start(metadata_offset))
        .expect("Failed to seek to metadata");
    let mut metadata_buf = vec![0; metadata_length as usize];
    file.read_exact(&mut metadata_buf)
        .expect("Failed to read metadata");

    let metadata: Vec<MetadataEntry> =
        serde_json::from_slice(&metadata_buf).expect("Failed to deserialize metadata");

    let mut files = HashMap::new();
    let mut manifest = None;

    for entry in metadata {
        file.seek(SeekFrom::Start(entry.offset))
            .expect("Failed to seek to file data");
        let mut data = vec![0; entry.size];
        file.read_exact(&mut data)
            .expect("Failed to read file data");

        if entry.name == "manifest" {
            manifest = Some(serde_json::from_slice(&data).expect("Failed to deserialize manifest"));
        } else {
            files.insert(entry.name, data);
        }
    }

    let manifest = manifest.expect("Manifest not found in executable");

    SetupPackage { manifest, files }
}

impl SetupPackage {
    /// Extracts a file from a packaged executable
    pub fn get_file(&self, filename: &str) -> Vec<u8> {
        self.files
            .get(filename)
            .expect("File not found in executable")
            .clone()
    }

    pub fn get_application(&self) -> Vec<u8> {
        let filename = &self.manifest.application;
        self.get_file(&filename)
    }

    pub fn get_webview2(&self) -> Option<Vec<u8>> {
        self.files.get(WEBVIEW2_EVERGREEN_EXE).cloned()
    }

    pub fn webview2_filename(&self) -> String {
        WEBVIEW2_EVERGREEN_EXE.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_extract_package() {
        let exe_data = vec![0; 1000]; // Mock executable data
        let mut packager = ExePackager::new(exe_data.clone());

        // Add files and manifest
        packager.add_file("file1.txt", b"Hello, world!".to_vec());
        packager.add_file("file2.txt", b"Rust is awesome!".to_vec());

        let manifest = SetupManifest {
            name: "TestApp".to_string(),
            title: "Test App".to_string(),
            version: "1.0.0".to_string(),
            identifier: "com.example.testapp".to_string(),
            application: "test.exe".to_string(),
        };

        packager.add_manifest(&manifest);

        // Create a temporary directory
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.exe");

        // Package the executable
        packager.package(&output_path);

        // Extract the packaged executable
        let setup_package = extract_package(&output_path);
        let extracted_manifest = setup_package.manifest;
        let extracted_files = setup_package.files;

        // Check the extracted manifest
        assert_eq!(extracted_manifest.name, "TestApp");
        assert_eq!(extracted_manifest.version, "1.0.0");
        assert_eq!(extracted_manifest.identifier, "com.example.testapp");

        // Check the extracted files
        assert_eq!(
            extracted_files.get("file1.txt").unwrap(),
            &b"Hello, world!".to_vec()
        );
        assert_eq!(
            extracted_files.get("file2.txt").unwrap(),
            &b"Rust is awesome!".to_vec()
        );
    }

    #[test]
    fn test_extract_package_no_manifest() {
        let exe_data = vec![0; 1000]; // Mock executable data
        let mut packager = ExePackager::new(exe_data.clone());

        // Add files without manifest
        packager.add_file("file1.txt", b"Hello, world!".to_vec());
        packager.add_file("file2.txt", b"Rust is awesome!".to_vec());

        // Create a temporary directory
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.exe");

        // Package the executable
        packager.package(&output_path);

        // Extract the packaged executable
        let result = std::panic::catch_unwind(|| {
            extract_package(&output_path);
        });

        // Check that the extraction fails because there is no manifest
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_package_empty_exe() {
        // Create an empty executable file
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("empty.exe");

        let mut file = File::create(&output_path).expect("Failed to create empty executable");
        file.write_all(&[])
            .expect("Failed to write to empty executable");

        // Extract the packaged executable
        let result = std::panic::catch_unwind(|| {
            extract_package(&output_path);
        });

        // Check that the extraction fails because the executable is empty
        assert!(result.is_err());
    }
}
