use crate::metadata::{MetadataEntry, METADATA_OFFSET_SIZE};

use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// A packager for bundling files with an executable.
///
/// Example usage:
/// ```no_run
/// use bundler::exe_packager::{ExePackager, SetupManifest};
/// use std::path::Path;
///
/// fn main() {
///     let setup_data = std::fs::read("setup.exe").expect("Failed to read setup.exe");
///     let mut packager = ExePackager::new(setup_data);
///     packager.add_file("file1.txt", b"Hello, world!".to_vec());
///     packager.add_file("file2.txt", b"Rust is awesome!".to_vec());
///
///     let manifest = SetupManifest {
///         name: "MyApp".to_string(),
///         version: "1.0.0".to_string(),
///         identifier: "com.example.myapp".to_string(),
///         application: "myapp.exe".to_string(),
///     };
///     packager.add_manifest(&manifest);
///     packager.package(Path::new("output.exe"));
/// }
/// ```
/// This will create an `output.exe` file that contains the `setup.exe`, `file1.txt`, `file2.txt`, and a manifest.
/// The manifest can be extracted from the `output.exe` file using the `extract_package` function.
pub struct ExePackager {
    exe_data: Vec<u8>,
    files: HashMap<String, Vec<u8>>,
    manifest: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SetupManifest {
    pub name: String,
    pub version: String,
    pub identifier: String,
    pub application: String,
}

impl ExePackager {
    /// Creates a new ExePackager with the given executable data
    pub fn new(exe_data: Vec<u8>) -> Self {
        ExePackager {
            exe_data,
            files: HashMap::new(),
            manifest: None,
        }
    }

    /// Adds a file to be packaged with the executable
    pub fn add_file(&mut self, filename: &str, file_data: Vec<u8>) {
        self.files.insert(filename.to_string(), file_data);
    }

    /// Adds a manifest to be packaged with the executable
    pub fn add_manifest(&mut self, manifest: &SetupManifest) {
        let manifest_data = serde_json::to_vec(manifest).expect("Failed to serialize manifest");
        self.manifest = Some(manifest_data);
    }

    /// Finalizes the package by appending the files and manifest to the executable
    pub fn package(self, output_path: &Path) {
        let mut output_file = File::create(output_path).expect("Failed to create output file");

        // Write the original exe data
        output_file
            .write_all(&self.exe_data)
            .expect("Failed to write exe data");

        // Metadata to keep track of the files and manifest
        let mut metadata = Vec::new();

        // Write the manifest if it exists
        if let Some(manifest_data) = self.manifest {
            let offset = output_file
                .seek(SeekFrom::Current(0))
                .expect("Failed to get current position");
            output_file
                .write_all(&manifest_data)
                .expect("Failed to write manifest data");
            let manifest_metadata = MetadataEntry {
                name: "manifest".to_string(),
                offset,
                size: manifest_data.len(),
            };
            metadata.push(manifest_metadata);
        }

        for (filename, file_data) in self.files {
            // Write each file
            let offset = output_file
                .seek(SeekFrom::Current(0))
                .expect("Failed to get current position");
            output_file
                .write_all(&file_data)
                .expect("Failed to write file data");

            // Store metadata as filename, offset, and size
            let file_metadata = MetadataEntry {
                name: filename,
                offset,
                size: file_data.len(),
            };
            metadata.push(file_metadata);
        }

        // Serialize and write the metadata
        let metadata_data = serde_json::to_vec(&metadata).expect("Failed to serialize metadata");
        let metadata_offset = output_file
            .seek(SeekFrom::Current(0))
            .expect("Failed to get current position");
        output_file
            .write_all(&metadata_data)
            .expect("Failed to write metadata");

        // Write the offset of the metadata at the end of the file
        let metadata_offset_str =
            format!("{:0width$}", metadata_offset, width = METADATA_OFFSET_SIZE);
        output_file
            .write_all(metadata_offset_str.as_bytes())
            .expect("Failed to write metadata offset");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::tempdir;

    #[test]
    fn test_add_file() {
        let exe_data = vec![0; 1000]; // Mock executable data
        let mut packager = ExePackager::new(exe_data);

        packager.add_file("test.txt", b"Hello, world!".to_vec());
        assert!(packager.files.contains_key("test.txt"));
    }

    #[test]
    fn test_add_manifest() {
        let exe_data = vec![0; 1000]; // Mock executable data
        let mut packager = ExePackager::new(exe_data);

        let manifest = SetupManifest {
            name: "TestApp".to_string(),
            version: "1.0.0".to_string(),
            identifier: "com.example.testapp".to_string(),
            application: "test.exe".to_string(),
        };

        packager.add_manifest(&manifest);
        assert!(packager.manifest.is_some());
    }

    #[test]
    fn test_package() {
        let exe_data = vec![0; 1000]; // Mock executable data
        let mut packager = ExePackager::new(exe_data);

        // Add files and manifest
        packager.add_file("file1.txt", b"Hello, world!".to_vec());
        packager.add_file("file2.txt", b"Rust is awesome!".to_vec());

        let manifest = SetupManifest {
            name: "TestApp".to_string(),
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

        // Verify the contents of the packaged executable
        let mut packaged_file = File::open(&output_path).expect("Failed to open packaged file");
        let mut packaged_data = Vec::new();
        packaged_file
            .read_to_end(&mut packaged_data)
            .expect("Failed to read packaged file");

        // Check the original exe data
        assert_eq!(&packaged_data[..1000], &[0; 1000]);

        // Check the appended files and manifest
        let metadata_offset = &packaged_data[packaged_data.len() - 16..];
        let metadata_offset: u64 = String::from_utf8_lossy(metadata_offset)
            .trim()
            .parse()
            .expect("Failed to parse metadata offset");

        let metadata: Vec<MetadataEntry> = {
            let mut metadata_file =
                &packaged_data[metadata_offset as usize..packaged_data.len() - 16];
            let metadata_json = String::from_utf8_lossy(&mut metadata_file);
            serde_json::from_str(&metadata_json).expect("Failed to deserialize metadata")
        };

        for entry in metadata {
            let data = &packaged_data[entry.offset as usize..entry.offset as usize + entry.size];
            match entry.name.as_str() {
                "manifest" => {
                    let manifest: SetupManifest =
                        serde_json::from_slice(data).expect("Failed to deserialize manifest");
                    assert_eq!(manifest.name, "TestApp");
                    assert_eq!(manifest.version, "1.0.0");
                    assert_eq!(manifest.identifier, "com.example.testapp");
                    assert_eq!(manifest.application, "test.exe");
                }
                "file1.txt" => assert_eq!(data, b"Hello, world!"),
                "file2.txt" => assert_eq!(data, b"Rust is awesome!"),
                _ => panic!("Unknown file in package: {}", entry.name),
            }
        }
    }
}
