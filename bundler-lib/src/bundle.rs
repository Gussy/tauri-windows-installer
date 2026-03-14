//! Library API for creating TWI setup executables.
//!
//! This module provides the [`bundle`] function and associated types for
//! programmatically creating Windows installer executables. The CLI bundler
//! is a thin wrapper around this API.
//!
//! # Example
//!
//! ```no_run
//! use bundler::bundle::{bundle, BundleOptions};
//! use std::path::PathBuf;
//!
//! let options = BundleOptions {
//!     setup_exe: std::fs::read("setup.exe").unwrap(),
//!     name: "my-app".into(),
//!     title: "My App".into(),
//!     version: "1.0.0".into(),
//!     identifier: "com.example.myapp".into(),
//!     publisher: "Example Inc.".into(),
//!     app: PathBuf::from("target/release/my-app.exe"),
//!     main_exe: None,
//!     icon: None,
//!     webview2: None,
//!     sign_command: None,
//!     output_dir: PathBuf::from("dist"),
//!     on_progress: None,
//! };
//!
//! let output = bundle(options).unwrap();
//! println!("Created installer at {}", output.path.display());
//! ```

use crate::manifest::SetupManifest;
use crate::{
    BUNDLE_RESOURCE, MANIFEST_RESOURCE, TWI_RESOURCE, WEBVIEW2_RESOURCE,
    WEBVIEW2_RESOURCE_FILENAME,
};
use std::path::{Path, PathBuf};
use std::{fs, process::Command};

/// Options for creating a setup executable.
pub struct BundleOptions {
    /// Raw bytes of the setup.exe stub to embed resources into.
    pub setup_exe: Vec<u8>,
    /// Product name used for the output filename (e.g. `"my-app"` → `my-app-setup.exe`).
    pub name: String,
    /// Human-readable application title shown in the installer UI.
    pub title: String,
    /// Semantic version string (e.g. `"1.2.3"`).
    pub version: String,
    /// Reverse-domain identifier (e.g. `"com.example.myapp"`).
    pub identifier: String,
    /// Publisher name shown in Windows "Add/Remove Programs".
    pub publisher: String,
    /// Path to the application executable or directory to bundle.
    pub app: PathBuf,
    /// Main executable name, required when [`app`](Self::app) is a directory.
    pub main_exe: Option<String>,
    /// Absolute path to a PNG icon file for the setup executable.
    pub icon: Option<PathBuf>,
    /// Optional WebView2 installer to embed.
    pub webview2: Option<WebView2Embedding>,
    /// Shell command used to sign the output executable.
    /// The output file path is appended as the last argument.
    pub sign_command: Option<String>,
    /// Directory where the output `{name}-setup.exe` will be written.
    pub output_dir: PathBuf,
    /// Optional callback invoked with progress messages.
    pub on_progress: Option<Box<dyn Fn(&str)>>,
}

/// WebView2 installer data to embed in the setup executable.
pub struct WebView2Embedding {
    /// Raw bytes of the WebView2 installer executable.
    pub data: Vec<u8>,
    /// Filename for the WebView2 installer (e.g. `"MicrosoftEdgeWebview2Setup.exe"`).
    pub filename: String,
}

/// Result of a successful bundle operation.
pub struct BundleOutput {
    /// Absolute path to the created setup executable.
    pub path: PathBuf,
    /// File size of the created setup executable in bytes.
    pub size: u64,
}

/// Errors that can occur during the bundle process.
#[derive(Debug)]
pub enum BundleError {
    /// I/O error (reading/writing files).
    Io(std::io::Error),
    /// Error manipulating the PE executable.
    Pe(String),
    /// Error creating the tar archive.
    Tar(String),
    /// Error compressing the bundle.
    Compress(String),
    /// Error signing the executable.
    Sign(String),
    /// Validation error in the provided options.
    Validation(String),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BundleError::Io(e) => write!(f, "I/O error: {}", e),
            BundleError::Pe(e) => write!(f, "PE error: {}", e),
            BundleError::Tar(e) => write!(f, "Tar error: {}", e),
            BundleError::Compress(e) => write!(f, "Compression error: {}", e),
            BundleError::Sign(e) => write!(f, "Signing error: {}", e),
            BundleError::Validation(e) => write!(f, "Validation error: {}", e),
        }
    }
}

impl std::error::Error for BundleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BundleError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for BundleError {
    fn from(e: std::io::Error) -> Self {
        BundleError::Io(e)
    }
}

/// Create a TWI setup executable from the given options.
///
/// This is the main entry point for programmatic bundling. The function:
/// 1. Embeds the application (file or directory) as a compressed tar archive
/// 2. Writes a setup manifest with metadata
/// 3. Optionally embeds a WebView2 installer and icon
/// 4. Sets PE version info
/// 5. Optionally signs the output executable
///
/// Returns a [`BundleOutput`] with the path and size of the created installer.
pub fn bundle(options: BundleOptions) -> Result<BundleOutput, BundleError> {
    let on_progress = options.on_progress;
    let progress = |msg: &str| {
        if let Some(ref cb) = on_progress {
            cb(msg);
        }
    };

    // Create a PortableExecutable from the setup data
    let mut setup_pe = libsui::PortableExecutable::from(&options.setup_exe)
        .map_err(|e| BundleError::Pe(e.to_string()))?;

    // Set icon if provided
    if let Some(ref icon_path) = options.icon {
        let icon_data = fs::read(icon_path)?;
        setup_pe = setup_pe
            .set_icon(&icon_data)
            .map_err(|e| BundleError::Pe(e.to_string()))?;
        progress(&format!("Added icon: {}", icon_path.display()));
    }

    // Create the tar bundle and determine the main executable name
    let app_path = &options.app;
    let (bundle_data, main_exe_name) = if app_path.is_dir() {
        let main_exe = options.main_exe.as_deref().ok_or_else(|| {
            BundleError::Validation(
                "--main-exe is required when --app is a directory".to_string(),
            )
        })?;

        // Verify the main exe exists in the directory
        if !app_path.join(main_exe).exists() {
            return Err(BundleError::Validation(format!(
                "Main executable '{}' not found in directory '{}'",
                main_exe,
                app_path.display()
            )));
        }

        let tar_data = create_tar_from_directory(app_path)?;
        let compressed = compress_bundle(&tar_data)?;
        progress(&format!(
            "Bundled directory: {} ({} bytes -> {} bytes, main exe: {})",
            app_path.display(),
            tar_data.len(),
            compressed.len(),
            main_exe
        ));
        (compressed, main_exe.to_string())
    } else {
        let exe_name = app_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                BundleError::Validation(format!(
                    "Could not extract filename from app path '{}'",
                    app_path.display()
                ))
            })?
            .to_string();
        let tar_data = create_tar_from_file(app_path, &exe_name)?;
        let compressed = compress_bundle(&tar_data)?;
        progress(&format!(
            "Bundled application: {} ({} bytes -> {} bytes)",
            exe_name,
            tar_data.len(),
            compressed.len(),
        ));
        (compressed, exe_name)
    };

    // Create the manifest
    let manifest = SetupManifest {
        name: options.name.clone(),
        title: options.title.clone(),
        version: options.version.clone(),
        identifier: options.identifier.clone(),
        application: main_exe_name,
        publisher: options.publisher.clone(),
    };

    // Write bundle data as a PE resource
    setup_pe = setup_pe
        .write_resource(BUNDLE_RESOURCE, bundle_data)
        .map_err(|e| BundleError::Pe(e.to_string()))?;

    // Write manifest as a PE resource
    setup_pe = setup_pe
        .write_resource(
            MANIFEST_RESOURCE,
            manifest
                .to_binary()
                .map_err(|e| BundleError::Pe(e.to_string()))?,
        )
        .map_err(|e| BundleError::Pe(e.to_string()))?;

    // Handle WebView2 embedding
    if let Some(wv2) = options.webview2 {
        progress("Embedding WebView2 installer...");
        setup_pe = setup_pe
            .write_resource(WEBVIEW2_RESOURCE, wv2.data)
            .map_err(|e| BundleError::Pe(e.to_string()))?;
        setup_pe = setup_pe
            .write_resource(
                WEBVIEW2_RESOURCE_FILENAME,
                wv2.filename.into_bytes(),
            )
            .map_err(|e| BundleError::Pe(e.to_string()))?;
    }

    // Write the TWI marker resource
    setup_pe = setup_pe
        .write_resource(TWI_RESOURCE, TWI_RESOURCE.as_bytes().to_vec())
        .map_err(|e| BundleError::Pe(e.to_string()))?;

    // Build the output executable
    let output_filename = format!("{}-setup.exe", options.name);
    let output_path = options.output_dir.join(&output_filename);
    fs::create_dir_all(&options.output_dir)?;
    let mut output_file = fs::File::create(&output_path)?;
    setup_pe
        .build(&mut output_file)
        .map_err(|e| BundleError::Pe(e.to_string()))?;
    drop(output_file);

    // Embed PE version info resources
    set_version_info(&output_path, &manifest)?;
    progress(&format!("Set version info: {}", manifest.version));

    // Sign the output executable if a sign command is provided
    if let Some(ref sign_cmd) = options.sign_command {
        sign_executable(sign_cmd, &output_path)?;
        progress("Signing successful.");
    }

    let output_size = fs::metadata(&output_path)?.len();

    Ok(BundleOutput {
        path: output_path,
        size: output_size,
    })
}

fn compress_bundle(data: &[u8]) -> Result<Vec<u8>, BundleError> {
    // zstd level 19 — high compression, only runs once at build time
    zstd::encode_all(data, 19).map_err(|e| BundleError::Compress(e.to_string()))
}

fn create_tar_from_file(file_path: &Path, name: &str) -> Result<Vec<u8>, BundleError> {
    let data = fs::read(file_path)?;
    let mut builder = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();

    builder
        .append_data(&mut header, name, data.as_slice())
        .map_err(|e| BundleError::Tar(e.to_string()))?;
    builder
        .into_inner()
        .map_err(|e| BundleError::Tar(e.to_string()))
}

fn create_tar_from_directory(dir_path: &Path) -> Result<Vec<u8>, BundleError> {
    let mut builder = tar::Builder::new(Vec::new());
    builder
        .append_dir_all(".", dir_path)
        .map_err(|e| BundleError::Tar(e.to_string()))?;
    builder
        .into_inner()
        .map_err(|e| BundleError::Tar(e.to_string()))
}

fn set_version_info(output_path: &Path, manifest: &SetupManifest) -> Result<(), BundleError> {
    use editpe::types::FixedFileInfo;
    use editpe::Image;

    let pe_data = fs::read(output_path)?;
    let mut image =
        Image::parse(pe_data).map_err(|e| BundleError::Pe(format!("Failed to parse PE: {}", e)))?;

    let mut resources = image.resource_directory().cloned().unwrap_or_default();

    let version = parse_version_u32(&manifest.version);

    let version_info = editpe::VersionInfo {
        info: FixedFileInfo {
            file_version: version,
            product_version: version,
            ..FixedFileInfo::default()
        },
        strings: vec![editpe::VersionStringTable {
            key: "040904B0".to_string(),
            strings: indexmap::indexmap! {
                "CompanyName".to_string() => manifest.publisher.clone(),
                "FileDescription".to_string() => format!("{} Setup", manifest.title),
                "FileVersion".to_string() => manifest.version.clone(),
                "InternalName".to_string() => format!("{}-setup", manifest.name),
                "OriginalFilename".to_string() => format!("{}-setup.exe", manifest.name),
                "ProductName".to_string() => manifest.title.clone(),
                "ProductVersion".to_string() => manifest.version.clone(),
            },
        }],
        vars: vec![],
    };

    resources
        .set_version_info(&version_info)
        .map_err(|e| BundleError::Pe(format!("Failed to set version info: {}", e)))?;
    image
        .set_resource_directory(resources)
        .map_err(|e| BundleError::Pe(format!("Failed to set resource directory: {}", e)))?;

    fs::write(output_path, image.data())?;

    Ok(())
}

fn parse_version_u32(version_str: &str) -> editpe::types::VersionU32 {
    let parts: Vec<u16> = version_str
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();

    let major = *parts.first().unwrap_or(&0);
    let minor = *parts.get(1).unwrap_or(&0);
    let patch = *parts.get(2).unwrap_or(&0);
    let build = *parts.get(3).unwrap_or(&0);

    editpe::types::VersionU32 {
        major: ((major as u32) << 16) | minor as u32,
        minor: ((patch as u32) << 16) | build as u32,
    }
}

fn sign_executable(sign_cmd: &str, file_path: &Path) -> Result<(), BundleError> {
    let file_path_str = file_path
        .to_str()
        .ok_or_else(|| BundleError::Sign("Output path contains invalid UTF-8".to_string()))?;

    // Parse the sign command — first token is the program, rest are arguments
    let mut parts = shell_words::split(sign_cmd)
        .map_err(|e| BundleError::Sign(format!("Failed to parse sign command: {}", e)))?;

    if parts.is_empty() {
        return Err(BundleError::Sign("Sign command is empty".to_string()));
    }

    let program = parts.remove(0);
    parts.push(file_path_str.to_string());

    let output = Command::new(&program)
        .args(&parts)
        .output()
        .map_err(|e| BundleError::Sign(format!("Failed to execute '{}': {}", program, e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(BundleError::Sign(format!(
            "Sign command failed (exit code {}):\nstdout: {}\nstderr: {}",
            output.status.code().unwrap_or(-1),
            stdout.trim(),
            stderr.trim()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_parse_version_u32_full() {
        let v = parse_version_u32("1.2.3");
        assert_eq!(v.major, (1 << 16) | 2);
        assert_eq!(v.minor, (3 << 16) | 0);
    }

    #[test]
    fn test_parse_version_u32_with_build() {
        let v = parse_version_u32("10.20.30.40");
        assert_eq!(v.major, (10 << 16) | 20);
        assert_eq!(v.minor, (30 << 16) | 40);
    }

    #[test]
    fn test_parse_version_u32_short() {
        let v = parse_version_u32("5");
        assert_eq!(v.major, 5 << 16);
        assert_eq!(v.minor, 0);
    }

    #[test]
    fn test_parse_version_u32_empty() {
        let v = parse_version_u32("");
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 0);
    }

    #[test]
    fn test_create_tar_from_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.exe");
        fs::write(&file_path, b"hello world").unwrap();

        let tar_data = create_tar_from_file(&file_path, "test.exe").unwrap();

        let mut archive = tar::Archive::new(tar_data.as_slice());
        let mut entries = archive.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();

        assert_eq!(entry.path().unwrap().to_str().unwrap(), "test.exe");
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();
        assert_eq!(contents, b"hello world");
    }

    #[test]
    fn test_create_tar_from_directory_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("app.exe"), b"binary").unwrap();
        fs::write(dir.path().join("config.toml"), b"[settings]").unwrap();

        let tar_data = create_tar_from_directory(dir.path()).unwrap();

        let mut archive = tar::Archive::new(tar_data.as_slice());
        let entries: Vec<_> = archive
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.header().entry_type().is_file())
            .map(|e| e.path().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(
            entries.iter().any(|e| e.ends_with("app.exe")),
            "Expected app.exe in entries: {:?}",
            entries
        );
        assert!(
            entries.iter().any(|e| e.ends_with("config.toml")),
            "Expected config.toml in entries: {:?}",
            entries
        );
    }

    #[test]
    fn test_compress_bundle_roundtrip() {
        let original = b"some data to compress for testing purposes";
        let compressed = compress_bundle(original).unwrap();

        assert!(!compressed.is_empty());

        let decompressed = zstd::decode_all(compressed.as_slice()).unwrap();
        assert_eq!(decompressed, original);
    }
}
