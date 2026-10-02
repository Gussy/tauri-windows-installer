//! Library API for creating TWI setup executables.
//!
//! This module provides the [`bundle`] function and associated types for
//! programmatically creating Windows installer executables. The CLI bundler
//! is a thin wrapper around this API.
//!
//! # Example
//!
//! ```no_run
//! use twi_core::bundle::{bundle, BundleOptions};
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
//!     desktop_shortcut: true,
//!     on_progress: None,
//! };
//!
//! let output = bundle(options).unwrap();
//! println!("Created installer at {}", output.path.display());
//! ```

use crate::manifest::SetupManifest;
use crate::{
    BUNDLE_RESOURCE, MANIFEST_RESOURCE, TWI_RESOURCE, WEBVIEW2_RESOURCE, WEBVIEW2_RESOURCE_FILENAME,
};
use sha2::{Digest, Sha256};
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
    /// Signing program and literal arguments. `%1` is substituted in place;
    /// without a placeholder, the output path is appended.
    pub sign_command: Option<SigningCommand>,
    /// Directory where the output `{name}-setup.exe` will be written.
    pub output_dir: PathBuf,
    /// Whether to create a desktop shortcut during installation.
    pub desktop_shortcut: bool,
    /// Optional callback invoked with progress messages.
    #[allow(clippy::type_complexity)]
    pub on_progress: Option<Box<dyn Fn(&str)>>,
}

/// A signing executable and its literal arguments. No shell is invoked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl SigningCommand {
    /// Parse the legacy command notation, retaining Windows path backslashes.
    /// Prefer explicit `program`/`args` when arguments contain complex quoting.
    pub fn parse_legacy(command: &str) -> Result<Self, BundleError> {
        let mut words = Vec::new();
        let mut word = String::new();
        let mut quote = None;
        let mut started = false;
        let mut chars = command.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' && quote != Some('\'') {
                let mut count = 1;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    count += 1;
                }
                if chars.peek() == Some(&'"') {
                    chars.next();
                    word.extend(std::iter::repeat_n('\\', count / 2));
                    if count % 2 == 1 {
                        word.push('"');
                    } else if quote == Some('"') {
                        quote = None;
                    } else {
                        quote = Some('"');
                    }
                } else {
                    word.extend(std::iter::repeat_n('\\', count));
                }
                started = true;
            } else if (c == '"' || c == '\'') && (quote.is_none() || quote == Some(c)) {
                quote = if quote == Some(c) { None } else { Some(c) };
                started = true;
            } else if c.is_whitespace() && quote.is_none() {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            } else {
                word.push(c);
                started = true;
            }
        }
        if quote.is_some() {
            return Err(BundleError::Sign(
                "Unclosed quote in signing command".into(),
            ));
        }
        if started {
            words.push(word);
        }
        if words.is_empty() || words[0].is_empty() {
            return Err(BundleError::Sign("Signing program is empty".into()));
        }
        Ok(Self {
            program: PathBuf::from(words.remove(0)),
            args: words,
        })
    }

    fn arguments(&self, path: &Path) -> Result<Vec<String>, BundleError> {
        let path = path
            .to_str()
            .ok_or_else(|| BundleError::Sign("Output path is not UTF-8".into()))?;
        let has_placeholder = self.args.iter().any(|arg| arg.contains("%1"));
        let mut args: Vec<_> = self
            .args
            .iter()
            .map(|arg| arg.replace("%1", path))
            .collect();
        if !has_placeholder {
            args.push(path.into());
        }
        Ok(args)
    }
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
    crate::validate_identifier(&options.identifier).map_err(BundleError::Validation)?;
    crate::validate_windows_filename(&options.name).map_err(BundleError::Validation)?;
    crate::validate_windows_filename(&options.title).map_err(BundleError::Validation)?;
    validate_setup_stub(&options.setup_exe)?;
    parse_version_u32(&options.version)?;
    let on_progress = options.on_progress;
    let progress = |msg: &str| {
        if let Some(ref cb) = on_progress {
            cb(msg);
        }
    };

    // Create the tar bundle and determine the main executable name
    let app_path = &options.app;
    let (bundle_data, main_exe_name, unpacked_size) = if app_path.is_dir() {
        let main_exe = options.main_exe.as_deref().ok_or_else(|| {
            BundleError::Validation("--main-exe is required when --app is a directory".to_string())
        })?;

        crate::validate_relative_executable(main_exe).map_err(BundleError::Validation)?;
        let normalized_main = main_exe.replace('\\', "/");
        let main_path = app_path.join(&normalized_main);
        // The archive rejects links, including links in ancestor directories.
        if !fs::symlink_metadata(&main_path)
            .map(|m| m.is_file() && !m.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(BundleError::Validation(format!(
                "Main executable '{}' not found in directory '{}'",
                main_exe,
                app_path.display()
            )));
        }

        validate_application(&fs::read(&main_path)?)?;
        let tar_data = create_tar_from_directory(app_path)?;
        let unpacked_size = archive_size(&tar_data)?;
        let compressed = compress_bundle(&tar_data)?;
        progress(&format!(
            "Bundled directory: {} ({} bytes -> {} bytes, main exe: {})",
            app_path.display(),
            tar_data.len(),
            compressed.len(),
            main_exe
        ));
        (compressed, normalized_main, unpacked_size)
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
        crate::validate_relative_executable(&exe_name).map_err(BundleError::Validation)?;
        validate_application(&fs::read(app_path)?)?;
        let tar_data = create_tar_from_file(app_path, &exe_name)?;
        let unpacked_size = archive_size(&tar_data)?;
        let compressed = compress_bundle(&tar_data)?;
        progress(&format!(
            "Bundled application: {} ({} bytes -> {} bytes)",
            exe_name,
            tar_data.len(),
            compressed.len(),
        ));
        (compressed, exe_name, unpacked_size)
    };

    let bundle_sha256 = format!("{:x}", Sha256::digest(&bundle_data));
    // Create the manifest
    let manifest = SetupManifest {
        name: options.name.clone(),
        title: options.title.clone(),
        version: options.version.clone(),
        identifier: options.identifier.clone(),
        application: main_exe_name,
        publisher: options.publisher.clone(),
        desktop_shortcut: options.desktop_shortcut,
        unpacked_size,
        bundle_sha256,
    };
    crate::validate_manifest(&manifest).map_err(BundleError::Validation)?;

    // Create a PortableExecutable from the setup data
    // Any signature on the bare stub becomes invalid when its resources change.
    // Clear its certificate directory so a no-op signer cannot pass verification
    // using a stale certificate left over from the input stub.
    let mut unsigned_stub = options.setup_exe.clone();
    clear_certificate_table(&mut unsigned_stub)?;
    let mut setup_pe = libsui::PortableExecutable::from(&unsigned_stub)
        .map_err(|e| BundleError::Pe(e.to_string()))?;

    // Set icon if provided
    if let Some(ref icon_path) = options.icon {
        let icon_data = fs::read(icon_path)?;
        setup_pe = setup_pe
            .set_icon(&icon_data)
            .map_err(|e| BundleError::Pe(e.to_string()))?;
        progress(&format!("Added icon: {}", icon_path.display()));
    }

    // Write PE resources
    setup_pe = setup_pe
        .write_resource(BUNDLE_RESOURCE, bundle_data)
        .map_err(|e| BundleError::Pe(e.to_string()))?;

    setup_pe = setup_pe
        .write_resource(
            MANIFEST_RESOURCE,
            manifest
                .to_binary()
                .map_err(|e| BundleError::Pe(e.to_string()))?,
        )
        .map_err(|e| BundleError::Pe(e.to_string()))?;

    setup_pe = setup_pe
        .write_resource(TWI_RESOURCE, TWI_RESOURCE.as_bytes().to_vec())
        .map_err(|e| BundleError::Pe(e.to_string()))?;

    let expected_webview2 = options.webview2.as_ref().map(|wv2| {
        (
            format!("{:x}", Sha256::digest(&wv2.data)),
            wv2.filename.clone(),
        )
    });
    if let Some(wv2) = options.webview2 {
        crate::validate_windows_filename(&wv2.filename).map_err(BundleError::Validation)?;
        validate_pe(&wv2.data, false)?;
        progress("Embedding WebView2 installer...");
        setup_pe = setup_pe
            .write_resource(WEBVIEW2_RESOURCE, wv2.data)
            .map_err(|e| BundleError::Pe(e.to_string()))?;
        setup_pe = setup_pe
            .write_resource(WEBVIEW2_RESOURCE_FILENAME, wv2.filename.into_bytes())
            .map_err(|e| BundleError::Pe(e.to_string()))?;
    }

    // Set version info directly on libsui's resource directory before build().
    // This ensures icons, custom resources, and version info are all serialized
    // in a single pass, avoiding editpe round-trip corruption of icon data.
    set_version_info(setup_pe.resource_dir_mut(), &manifest)?;
    progress(&format!("Set version info: {}", manifest.version));

    // Build and sign privately in the destination filesystem. A failed build,
    // verification or signing operation cannot replace a previous release.
    fs::create_dir_all(&options.output_dir)?;
    let output_dir = fs::canonicalize(&options.output_dir)?;
    let output_path = output_dir.join(format!("{}-setup.exe", options.name));
    let work_dir = tempfile::tempdir_in(&output_dir)?;
    let staged_path = work_dir.path().join(format!("{}-setup.exe", options.name));
    let mut output_file = fs::File::create(&staged_path)?;
    setup_pe
        .build(&mut output_file)
        .map_err(|e| BundleError::Pe(e.to_string()))?;
    output_file.sync_all()?;
    drop(output_file);
    if let Some(ref sign_cmd) = options.sign_command {
        sign_executable(sign_cmd, &staged_path)?;
        validate_certificate_table(&fs::read(&staged_path)?)?;
        progress("Signing successful.");
    }
    verify_output(
        &fs::read(&staged_path)?,
        &manifest,
        &options.setup_exe,
        expected_webview2.as_ref(),
        options.icon.is_none(),
    )?;
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&staged_path)?
        .sync_all()?;
    // NamedTempFile::persist replaces an existing destination atomically on both
    // Windows and Unix. It does not remove the old output first.
    let publish = tempfile::NamedTempFile::new_in(&output_dir)?;
    fs::copy(&staged_path, publish.path())?;
    publish.as_file().sync_all()?;
    publish
        .persist(&output_path)
        .map_err(|e| BundleError::Io(e.error))?;
    let output_size = fs::metadata(&output_path)?.len();
    Ok(BundleOutput {
        path: output_path,
        size: output_size,
    })
}

/// Validate a Windows executable before invoking PE parsers or executing it.
pub fn validate_pe(data: &[u8], require_x64: bool) -> Result<(), BundleError> {
    let invalid = || BundleError::Pe("Invalid or truncated Windows executable".into());
    if data.len() < 64 || data.get(..2) != Some(b"MZ") {
        return Err(invalid());
    }
    let offset = u32::from_le_bytes(data[60..64].try_into().unwrap()) as usize;
    let header = data
        .get(offset..offset.checked_add(24).ok_or_else(invalid)?)
        .ok_or_else(invalid)?;
    if &header[..4] != b"PE\0\0" {
        return Err(invalid());
    }
    let machine = u16::from_le_bytes(header[4..6].try_into().unwrap());
    if require_x64 && machine != 0x8664 {
        return Err(BundleError::Validation(
            "Setup stub must target Windows x86_64".into(),
        ));
    }
    let sections = u16::from_le_bytes(header[6..8].try_into().unwrap()) as usize;
    let optional = u16::from_le_bytes(header[20..22].try_into().unwrap()) as usize;
    let section_start = offset.checked_add(24 + optional).ok_or_else(invalid)?;
    let tables = data
        .get(
            section_start
                ..section_start
                    .checked_add(sections * 40)
                    .ok_or_else(invalid)?,
        )
        .ok_or_else(invalid)?;
    if sections == 0 || optional < 96 {
        return Err(invalid());
    }
    let optional_header = data.get(offset + 24..section_start).ok_or_else(invalid)?;
    let magic = u16::from_le_bytes(optional_header[..2].try_into().unwrap());
    if !matches!(magic, 0x10b | 0x20b)
        || (require_x64 && magic != 0x20b)
        || (magic == 0x20b && optional < 112)
        || (matches!(machine, 0x8664 | 0xaa64) && magic != 0x20b)
        || (machine == 0x14c && magic != 0x10b)
    {
        return Err(invalid());
    }
    let directory_start = if magic == 0x20b { 112 } else { 96 };
    let directory_count = u32::from_le_bytes(
        optional_header[directory_start - 4..directory_start]
            .try_into()
            .unwrap(),
    ) as usize;
    if directory_count > 16 || optional < directory_start + directory_count * 8 {
        return Err(invalid());
    }
    let characteristics = u16::from_le_bytes(header[22..24].try_into().unwrap());
    if characteristics & 2 == 0 || characteristics & 0x2000 != 0 {
        return Err(BundleError::Pe(
            "Expected an executable PE, not a DLL".into(),
        ));
    }
    for section in tables.chunks_exact(40) {
        let size = u32::from_le_bytes(section[16..20].try_into().unwrap()) as usize;
        let start = u32::from_le_bytes(section[20..24].try_into().unwrap()) as usize;
        if start
            .checked_add(size)
            .filter(|end| *end <= data.len())
            .is_none()
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn validate_application(data: &[u8]) -> Result<(), BundleError> {
    validate_pe(data, false)?;
    let offset = u32::from_le_bytes(data[60..64].try_into().unwrap()) as usize;
    let machine = u16::from_le_bytes(data[offset + 4..offset + 6].try_into().unwrap());
    if !matches!(machine, 0x8664 | 0xaa64) {
        return Err(BundleError::Validation(
            "Main application must target Windows x86_64 or ARM64".into(),
        ));
    }
    let invalid = || {
        BundleError::Validation("Main executable has invalid loader headers or entry point".into())
    };
    let count = u16::from_le_bytes(data[offset + 6..offset + 8].try_into().unwrap()) as usize;
    let optional_size =
        u16::from_le_bytes(data[offset + 20..offset + 22].try_into().unwrap()) as usize;
    if !(64..=1024 * 1024).contains(&offset) || count > 96 || !(112..=4096).contains(&optional_size)
    {
        return Err(invalid());
    }
    let optional = offset + 24;
    let entry = u32::from_le_bytes(data[optional + 16..optional + 20].try_into().unwrap()) as u64;
    let image_size =
        u32::from_le_bytes(data[optional + 56..optional + 60].try_into().unwrap()) as u64;
    if entry == 0 || entry >= image_size {
        return Err(invalid());
    }
    let sections = optional + optional_size;
    let mut executable_entry = false;
    for section in data[sections..sections + count * 40].chunks_exact(40) {
        let virtual_size = u32::from_le_bytes(section[8..12].try_into().unwrap()) as u64;
        let address = u32::from_le_bytes(section[12..16].try_into().unwrap()) as u64;
        let raw_size = u32::from_le_bytes(section[16..20].try_into().unwrap()) as u64;
        let flags = u32::from_le_bytes(section[36..40].try_into().unwrap());
        let end = address + virtual_size.max(raw_size);
        if end > image_size {
            return Err(invalid());
        }
        executable_entry |= flags & 0x2000_0000 != 0 && entry >= address && entry < end;
    }
    if !executable_entry {
        return Err(invalid());
    }
    Ok(())
}

fn clear_certificate_table(data: &mut [u8]) -> Result<(), BundleError> {
    validate_pe(data, true)?;
    let pe = u32::from_le_bytes(data[60..64].try_into().unwrap()) as usize;
    let optional = pe + 24;
    let directories = u32::from_le_bytes(data[optional + 108..optional + 112].try_into().unwrap());
    if directories > 4 {
        data[optional + 112 + 4 * 8..optional + 112 + 5 * 8].fill(0);
    }
    Ok(())
}

fn validate_certificate_table(data: &[u8]) -> Result<(), BundleError> {
    validate_pe(data, true)?;
    let image = parse_pe(data)?;
    let invalid = || {
        BundleError::Sign(
            "Signing command produced an invalid Authenticode certificate table".into(),
        )
    };
    let directory = image
        .data_directory(editpe::DataDirectoryType::CertificateTable)
        .ok_or_else(invalid)?;
    let start = directory.virtual_address as usize;
    let end = start
        .checked_add(directory.size as usize)
        .ok_or_else(invalid)?;
    if start == 0 || !start.is_multiple_of(8) || directory.size < 9 || end > data.len() {
        return Err(invalid());
    }
    let mut cursor = start;
    while cursor < end {
        let header = data
            .get(cursor..cursor.checked_add(8).ok_or_else(invalid)?)
            .filter(|_| cursor + 8 <= end)
            .ok_or_else(invalid)?;
        let length = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        let revision = u16::from_le_bytes(header[4..6].try_into().unwrap());
        let kind = u16::from_le_bytes(header[6..8].try_into().unwrap());
        if length < 9
            || revision != 0x0200
            || kind != 0x0002
            || cursor
                .checked_add(length)
                .is_none_or(|entry_end| entry_end > end)
        {
            return Err(invalid());
        }
        let aligned = length.checked_add(7).ok_or_else(invalid)? & !7;
        cursor = cursor.checked_add(aligned).ok_or_else(invalid)?;
        if cursor > end {
            return Err(invalid());
        }
    }
    Ok(())
}

pub fn validate_setup_stub(data: &[u8]) -> Result<(), BundleError> {
    validate_pe(data, true)?;
    if !data
        .windows(crate::STUB_ABI_MARKER.len())
        .any(|w| w == crate::STUB_ABI_MARKER)
    {
        return Err(BundleError::Validation(
            "Setup stub is incompatible: rebuild twi_installer with this version of twi_core"
                .into(),
        ));
    }
    let image = parse_pe(data)?;
    if image
        .resource_directory()
        .is_some_and(|r| resource_bytes(r, TWI_RESOURCE).is_some())
    {
        return Err(BundleError::Validation(
            "Use a bare setup stub, not an already bundled installer".into(),
        ));
    }
    let manifest = image
        .resource_directory()
        .and_then(|r| r.get_manifest().ok().flatten())
        .ok_or_else(|| {
            BundleError::Validation("Setup stub is missing its Windows application manifest".into())
        })?;
    if !manifest.contains("asInvoker") {
        return Err(BundleError::Validation(
            "Setup stub must declare asInvoker".into(),
        ));
    }
    Ok(())
}

fn parse_pe(data: &[u8]) -> Result<editpe::Image<'_>, BundleError> {
    std::panic::catch_unwind(|| editpe::Image::parse(data))
        .map_err(|_| BundleError::Pe("Malformed PE resources".into()))?
        .map_err(|e| BundleError::Pe(e.to_string()))
}

fn resource_bytes<'a>(resources: &'a editpe::ResourceDirectory, name: &str) -> Option<&'a [u8]> {
    let editpe::ResourceEntry::Table(names) =
        resources.root().get(editpe::ResourceEntryName::ID(10))?
    else {
        return None;
    };
    let editpe::ResourceEntry::Table(languages) =
        names.get(editpe::ResourceEntryName::from_string(name))?
    else {
        return None;
    };
    let key = (*languages.entries().first()?).clone();
    let editpe::ResourceEntry::Data(data) = languages.get(key)? else {
        return None;
    };
    Some(data.data())
}

fn verify_output(
    data: &[u8],
    manifest: &SetupManifest,
    stub: &[u8],
    webview2: Option<&(String, String)>,
    preserve_icons: bool,
) -> Result<(), BundleError> {
    validate_pe(data, true)?;
    let image = parse_pe(data)?;
    let resources = image
        .resource_directory()
        .ok_or_else(|| BundleError::Pe("Output has no resource directory".into()))?;
    if resource_bytes(resources, TWI_RESOURCE) != Some(TWI_RESOURCE.as_bytes()) {
        return Err(BundleError::Pe(
            "Output installer marker missing or corrupted".into(),
        ));
    }
    if let Some((digest, filename)) = webview2 {
        let payload = resource_bytes(resources, WEBVIEW2_RESOURCE)
            .ok_or_else(|| BundleError::Pe("Output WebView2 resource missing".into()))?;
        if &format!("{:x}", Sha256::digest(payload)) != digest
            || resource_bytes(resources, WEBVIEW2_RESOURCE_FILENAME) != Some(filename.as_bytes())
        {
            return Err(BundleError::Pe(
                "Output WebView2 payload changed during signing".into(),
            ));
        }
    }
    let encoded = resource_bytes(resources, MANIFEST_RESOURCE)
        .ok_or_else(|| BundleError::Pe("Output manifest missing".into()))?;
    let decoded =
        SetupManifest::from_binary(encoded).map_err(|e| BundleError::Pe(e.to_string()))?;
    if &decoded != manifest {
        return Err(BundleError::Pe(
            "Output manifest changed during bundling/signing".into(),
        ));
    }
    let bundle = resource_bytes(resources, BUNDLE_RESOURCE)
        .ok_or_else(|| BundleError::Pe("Output archive missing".into()))?;
    if format!("{:x}", Sha256::digest(bundle)) != manifest.bundle_sha256 {
        return Err(BundleError::Pe(
            "Output archive checksum changed during bundling/signing".into(),
        ));
    }
    let original_image = parse_pe(stub)?;
    let original = original_image
        .resource_directory()
        .and_then(|r| r.get_manifest().ok().flatten());
    if resources
        .get_manifest()
        .map_err(|e| BundleError::Pe(e.to_string()))?
        != original
    {
        return Err(BundleError::Pe(
            "Windows application manifest was not preserved".into(),
        ));
    }
    if preserve_icons {
        for kind in [3, 14] {
            let original = original_image
                .resource_directory()
                .and_then(|r| r.root().get(editpe::ResourceEntryName::ID(kind)));
            if resources.root().get(editpe::ResourceEntryName::ID(kind)) != original {
                return Err(BundleError::Pe(
                    "Original setup icon resources were not preserved".into(),
                ));
            }
        }
    }
    Ok(())
}

fn archive_size(data: &[u8]) -> Result<u64, BundleError> {
    validate_archive_contract(data, crate::MAX_ARCHIVE_ENTRIES)
}

fn validate_archive_contract(data: &[u8], max_entries: usize) -> Result<u64, BundleError> {
    if data.len() as u64 > crate::MAX_ARCHIVE_SIZE {
        return Err(BundleError::Validation(
            "Decoded archive exceeds the supported 33 GiB limit".into(),
        ));
    }
    let mut archive = tar::Archive::new(data);
    let mut total = 0u64;
    for (index, entry) in archive
        .entries()
        .map_err(|e| BundleError::Tar(e.to_string()))?
        .raw(true)
        .enumerate()
    {
        if index >= max_entries {
            return Err(BundleError::Validation(
                "Archive has too many entries, including GNU long names".into(),
            ));
        }
        let entry = entry.map_err(|e| BundleError::Tar(e.to_string()))?;
        let kind = entry.header().entry_type();
        if kind.is_file() {
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| BundleError::Validation("Bundle size overflow".into()))?;
            if total > crate::MAX_UNPACKED_SIZE {
                return Err(BundleError::Validation(
                    "Installed application exceeds the supported 32 GiB limit".into(),
                ));
            }
        } else if !kind.is_dir() && !kind.is_gnu_longname() {
            return Err(BundleError::Validation(
                "Archive contains an unsupported entry".into(),
            ));
        }
    }
    // Include implied ancestors as well as explicit entries. This also handles
    // future archive writers that omit directory headers.
    let mut archive = tar::Archive::new(data);
    let mut paths = std::collections::HashSet::new();
    for entry in archive
        .entries()
        .map_err(|e| BundleError::Tar(e.to_string()))?
    {
        let entry = entry.map_err(|e| BundleError::Tar(e.to_string()))?;
        let path = entry.path().map_err(|e| BundleError::Tar(e.to_string()))?;
        let normalized = path.to_string_lossy().replace('\\', "/");
        crate::validate_archive_path_limits(&normalized).map_err(BundleError::Validation)?;
        for path in Path::new(&normalized)
            .ancestors()
            .filter(|path| !path.as_os_str().is_empty())
        {
            paths.insert(path.to_string_lossy().to_lowercase());
            if paths.len() > max_entries {
                return Err(BundleError::Validation(
                    "Archive expands to too many files and directories".into(),
                ));
            }
        }
    }
    Ok(total)
}

fn compress_bundle(data: &[u8]) -> Result<Vec<u8>, BundleError> {
    // zstd level 19 — high compression, only runs once at build time
    if data.len() as u64 > crate::MAX_ARCHIVE_SIZE {
        return Err(BundleError::Validation(
            "Decoded archive exceeds the supported size".into(),
        ));
    }
    let compressed =
        zstd::encode_all(data, 19).map_err(|e| BundleError::Compress(e.to_string()))?;
    crate::validate_archive_zstd_header(&compressed).map_err(BundleError::Validation)?;
    if compressed.len() > u32::MAX as usize {
        return Err(BundleError::Validation(
            "Compressed application exceeds the PE resource size limit".into(),
        ));
    }
    Ok(compressed)
}

fn append_file(
    builder: &mut tar::Builder<Vec<u8>>,
    file_path: &Path,
    name: &Path,
) -> Result<(), BundleError> {
    let metadata = fs::symlink_metadata(file_path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(BundleError::Validation(format!(
            "Only regular files can be bundled: {}",
            file_path.display()
        )));
    }
    let mut file = fs::File::open(file_path)?;
    if metadata.len() > crate::MAX_UNPACKED_SIZE
        || (builder.get_ref().len() as u64)
            .saturating_add(metadata.len())
            .saturating_add(512)
            > crate::MAX_ARCHIVE_SIZE
    {
        return Err(BundleError::Validation(
            "Application file exceeds the supported package size".into(),
        ));
    }
    let mut header = tar::Header::new_gnu();
    header.set_size(metadata.len());
    header.set_mode(0o755);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, name, &mut file)
        .map_err(|e| BundleError::Tar(e.to_string()))?;
    Ok(())
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = metadata;
        false
    }
}

fn create_tar_from_file(file_path: &Path, name: &str) -> Result<Vec<u8>, BundleError> {
    let mut builder = tar::Builder::new(Vec::new());
    append_file(&mut builder, file_path, Path::new(name))?;
    builder
        .into_inner()
        .map_err(|e| BundleError::Tar(e.to_string()))
}

fn create_tar_from_directory(dir_path: &Path) -> Result<Vec<u8>, BundleError> {
    let root = fs::symlink_metadata(dir_path)?;
    if root.file_type().is_symlink() || is_reparse_point(&root) {
        return Err(BundleError::Validation(
            "Bundle root must not be a link or reparse point".into(),
        ));
    }
    fn visit(
        builder: &mut tar::Builder<Vec<u8>>,
        root: &Path,
        dir: &Path,
        seen: &mut std::collections::HashSet<String>,
    ) -> Result<(), BundleError> {
        let remaining = crate::MAX_ARCHIVE_ENTRIES.saturating_sub(seen.len());
        let mut entries: Vec<_> = fs::read_dir(dir)?
            .take(remaining + 1)
            .collect::<Result<_, _>>()?;
        if entries.len() > remaining {
            return Err(BundleError::Validation(
                "Application directory has too many entries".into(),
            ));
        }
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
                return Err(BundleError::Validation(format!(
                    "Links and reparse points are not supported: {}",
                    path.display()
                )));
            }
            let name = entry.file_name();
            crate::validate_windows_filename(
                name.to_str()
                    .ok_or_else(|| BundleError::Validation("Archive names must be UTF-8".into()))?,
            )
            .map_err(BundleError::Validation)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|e| BundleError::Validation(e.to_string()))?;
            let relative_name = relative.to_string_lossy().replace('\\', "/");
            crate::validate_archive_path_limits(&relative_name).map_err(BundleError::Validation)?;
            let key = relative_name.to_lowercase();
            if key == crate::INSTALL_METADATA_FILENAME {
                return Err(BundleError::Validation(
                    "Application archive contains reserved installer metadata filename".into(),
                ));
            }
            if !seen.insert(key) {
                return Err(BundleError::Validation(
                    "Archive has Windows case-insensitive filename collisions".into(),
                ));
            }
            if seen.len() > crate::MAX_ARCHIVE_ENTRIES {
                return Err(BundleError::Validation(
                    "Application directory has too many entries".into(),
                ));
            }
            if metadata.is_dir() {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                header.set_uid(0);
                header.set_gid(0);
                header.set_mtime(0);
                header.set_cksum();
                builder
                    .append_data(&mut header, relative, std::io::empty())
                    .map_err(|e| BundleError::Tar(e.to_string()))?;
                visit(builder, root, &path, seen)?;
            } else {
                append_file(builder, &path, relative)?;
            }
        }
        Ok(())
    }
    let mut builder = tar::Builder::new(Vec::new());
    visit(
        &mut builder,
        dir_path,
        dir_path,
        &mut std::collections::HashSet::new(),
    )?;
    builder
        .into_inner()
        .map_err(|e| BundleError::Tar(e.to_string()))
}

/// Set version info directly on a resource directory.
/// Called on libsui's internal resource directory before build(), so icons,
/// custom resources, and version info are all serialized in a single pass.
fn set_version_info(
    resources: &mut editpe::ResourceDirectory,
    manifest: &SetupManifest,
) -> Result<(), BundleError> {
    use editpe::types::FixedFileInfo;

    let version = parse_version_u32(&manifest.version)?;

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

    Ok(())
}

fn parse_version_u32(version_str: &str) -> Result<editpe::types::VersionU32, BundleError> {
    let version = semver::Version::parse(version_str)
        .map_err(|e| BundleError::Validation(format!("Invalid semantic version: {e}")))?;
    let mut parts = [0u16; 3];
    for (out, value) in parts
        .iter_mut()
        .zip([version.major, version.minor, version.patch])
    {
        *out = u16::try_from(value).map_err(|_| {
            BundleError::Validation(
                "Version components must fit Windows 16-bit version fields".into(),
            )
        })?;
    }
    Ok(editpe::types::VersionU32 {
        major: ((parts[0] as u32) << 16) | parts[1] as u32,
        minor: (parts[2] as u32) << 16,
    })
}

fn sign_executable(sign_cmd: &SigningCommand, file_path: &Path) -> Result<(), BundleError> {
    if sign_cmd.program.as_os_str().is_empty() {
        return Err(BundleError::Sign("Signing program is empty".into()));
    }
    let mut child = Command::new(&sign_cmd.program)
        .args(sign_cmd.arguments(file_path)?)
        .spawn()
        .map_err(|e| BundleError::Sign(format!("Failed to execute signing program: {e}")))?;
    let started = std::time::Instant::now();
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BundleError::Sign(error.to_string()));
            }
        };
        if let Some(status) = status {
            return if status.success() {
                Ok(())
            } else {
                Err(BundleError::Sign(format!(
                    "Signing command failed with {status}"
                )))
            };
        }
        if started.elapsed() > std::time::Duration::from_secs(600) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(BundleError::Sign(
                "Signing command timed out after ten minutes".into(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_parse_version_u32_full() {
        let v = parse_version_u32("1.2.3").unwrap();
        assert_eq!(v.major, (1 << 16) | 2);
        assert_eq!(v.minor, 3 << 16);
    }

    #[test]
    fn test_parse_version_u32_with_build() {
        let v = parse_version_u32("10.20.30+40").unwrap();
        assert_eq!(v.major, (10 << 16) | 20);
        assert_eq!(v.minor, 30 << 16);
    }

    #[test]
    fn test_parse_version_u32_short() {
        assert!(parse_version_u32("5").is_err());
    }

    #[test]
    fn test_parse_version_u32_empty() {
        assert!(parse_version_u32("").is_err());
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

    #[test]
    fn test_bundle_error_display() {
        let io_err = BundleError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert!(io_err.to_string().contains("I/O error"));

        assert_eq!(
            BundleError::Pe("bad pe".into()).to_string(),
            "PE error: bad pe"
        );
        assert_eq!(
            BundleError::Tar("bad tar".into()).to_string(),
            "Tar error: bad tar"
        );
        assert_eq!(
            BundleError::Compress("bad zstd".into()).to_string(),
            "Compression error: bad zstd"
        );
        assert_eq!(
            BundleError::Sign("no cert".into()).to_string(),
            "Signing error: no cert"
        );
        assert_eq!(
            BundleError::Validation("missing field".into()).to_string(),
            "Validation error: missing field"
        );
    }

    #[test]
    fn test_bundle_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let bundle_err: BundleError = io_err.into();
        assert!(matches!(bundle_err, BundleError::Io(_)));
        assert!(bundle_err.to_string().contains("denied"));
    }

    #[test]
    fn test_bundle_rejects_empty_setup_exe() {
        let dir = tempfile::tempdir().unwrap();
        let options = BundleOptions {
            setup_exe: vec![],
            name: "test".into(),
            title: "Test".into(),
            version: "1.0.0".into(),
            identifier: "com.test".into(),
            publisher: "Test".into(),
            app: dir.path().join("app.exe"),
            main_exe: None,
            icon: None,
            webview2: None,
            sign_command: None,
            output_dir: dir.path().join("out"),
            desktop_shortcut: true,
            on_progress: None,
        };

        let result = bundle(options);
        assert!(result.is_err(), "bundle() should fail with empty setup_exe");
    }

    #[test]
    fn test_create_tar_from_file_nonexistent() {
        let result = create_tar_from_file(Path::new("/nonexistent/path.exe"), "path.exe");
        assert!(matches!(result, Err(BundleError::Io(_))));
    }
    fn put16(d: &mut [u8], p: usize, v: u16) {
        d[p..p + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(d: &mut [u8], p: usize, v: u32) {
        d[p..p + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put64(d: &mut [u8], p: usize, v: u64) {
        d[p..p + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn minimal_pe() -> Vec<u8> {
        let mut d = vec![0u8; 1024];
        put16(&mut d, 0, 0x5a4d);
        put32(&mut d, 0x3c, 0x80);
        put32(&mut d, 0x80, 0x4550);
        put16(&mut d, 0x84, 0x8664);
        put16(&mut d, 0x86, 1);
        put16(&mut d, 0x94, 240);
        put16(&mut d, 0x96, 0x22);
        let o = 0x98;
        put16(&mut d, o, 0x20b);
        put32(&mut d, o + 4, 512);
        put32(&mut d, o + 16, 0x1000);
        put32(&mut d, o + 20, 0x1000);
        put64(&mut d, o + 24, 0x140000000);
        put32(&mut d, o + 32, 4096);
        put32(&mut d, o + 36, 512);
        put16(&mut d, o + 40, 6);
        put16(&mut d, o + 48, 6);
        put32(&mut d, o + 56, 8192);
        put32(&mut d, o + 60, 512);
        put16(&mut d, o + 68, 2);
        put64(&mut d, o + 72, 1048576);
        put64(&mut d, o + 80, 4096);
        put64(&mut d, o + 88, 1048576);
        put64(&mut d, o + 96, 4096);
        put32(&mut d, o + 108, 16);
        let s = o + 240;
        d[s..s + 5].copy_from_slice(b".text");
        put32(&mut d, s + 8, 1);
        put32(&mut d, s + 12, 4096);
        put32(&mut d, s + 16, 512);
        put32(&mut d, s + 20, 512);
        put32(&mut d, s + 36, 0x60000020);
        d[512] = 0xc3;
        d
    }

    fn setup_fixture() -> Vec<u8> {
        let mut image = editpe::Image::parse(minimal_pe()).unwrap();
        let mut resources = editpe::ResourceDirectory::default();
        resources.root_mut().insert(
            editpe::ResourceEntryName::ID(24),
            editpe::ResourceEntry::Table(editpe::ResourceTable::default()),
        );
        resources.set_manifest(r#"<?xml version="1.0"?><assembly manifestVersion="1.0" xmlns="urn:schemas-microsoft-com:asm.v1"><trustInfo><security><requestedPrivileges><requestedExecutionLevel level="asInvoker" /></requestedPrivileges></security></trustInfo></assembly>"#).unwrap();
        image.set_resource_directory(resources).unwrap();
        let mut stub = Vec::new();
        libsui::PortableExecutable::from(image.data())
            .unwrap()
            .write_resource("TWI_SETUP_ABI", crate::STUB_ABI_MARKER.to_vec())
            .unwrap()
            .build(&mut stub)
            .unwrap();
        stub
    }
    fn setup_fixture_with_certificate() -> Vec<u8> {
        let mut stub = setup_fixture();
        stub.resize((stub.len() + 7) & !7, 0);
        let offset = stub.len();
        stub.extend_from_slice(&16u32.to_le_bytes());
        stub.extend_from_slice(&0x0200u16.to_le_bytes());
        stub.extend_from_slice(&0x0002u16.to_le_bytes());
        stub.extend_from_slice(&[1; 8]);
        let pe = u32::from_le_bytes(stub[60..64].try_into().unwrap()) as usize;
        put32(&mut stub, pe + 24 + 112 + 4 * 8, offset as u32);
        put32(&mut stub, pe + 24 + 112 + 4 * 8 + 4, 16);
        stub
    }
    const ICON_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4,
        0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 248, 15, 0, 1, 5,
        1, 1, 39, 24, 227, 102, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];
    fn setup_fixture_with_icon() -> Vec<u8> {
        let stub = setup_fixture();
        let mut image = libsui::PortableExecutable::from(&stub).unwrap();
        let mut group = vec![0, 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 32, 0];
        group.extend_from_slice(&(ICON_PNG.len() as u32).to_le_bytes());
        group.extend_from_slice(&7u16.to_le_bytes());
        for (kind, id, bytes) in [(3, 7, ICON_PNG.to_vec()), (14, 1, group)] {
            let mut payload = editpe::ResourceData::default();
            payload.set_data(bytes);
            let mut languages = editpe::ResourceTable::default();
            languages.insert(
                editpe::ResourceEntryName::ID(0),
                editpe::ResourceEntry::Data(payload),
            );
            let mut names = editpe::ResourceTable::default();
            names.insert(
                editpe::ResourceEntryName::ID(id),
                editpe::ResourceEntry::Table(languages),
            );
            image.resource_dir_mut().root_mut().insert(
                editpe::ResourceEntryName::ID(kind),
                editpe::ResourceEntry::Table(names),
            );
        }
        let mut data = Vec::new();
        image.build(&mut data).unwrap();
        data
    }
    fn bundle_options(root: &Path) -> BundleOptions {
        fs::write(root.join("app.exe"), minimal_pe()).unwrap();
        BundleOptions {
            setup_exe: setup_fixture(),
            name: "test-app".into(),
            title: "Test App".into(),
            version: "1.2.3-beta+build".into(),
            identifier: "com.example.test".into(),
            publisher: "Example".into(),
            app: root.join("app.exe"),
            main_exe: None,
            icon: None,
            webview2: None,
            sign_command: None,
            output_dir: root.join("out"),
            desktop_shortcut: false,
            on_progress: None,
        }
    }

    #[test]
    fn bundle_preserves_windows_manifest_and_verifies_complete_resources() {
        let dir = tempfile::tempdir().unwrap();
        let options = bundle_options(dir.path());
        let original = parse_pe(&options.setup_exe)
            .unwrap()
            .resource_directory()
            .unwrap()
            .get_manifest()
            .unwrap();
        let output = bundle(options).unwrap();
        assert!(output.path.is_absolute());
        let data = fs::read(output.path).unwrap();
        let image = parse_pe(&data).unwrap();
        let resources = image.resource_directory().unwrap();
        assert_eq!(resources.get_manifest().unwrap(), original);
        let manifest =
            SetupManifest::from_binary(resource_bytes(resources, MANIFEST_RESOURCE).unwrap())
                .unwrap();
        assert_eq!(manifest.unpacked_size, 1024);
        assert_eq!(
            manifest.bundle_sha256,
            format!(
                "{:x}",
                Sha256::digest(resource_bytes(resources, BUNDLE_RESOURCE).unwrap())
            )
        );
        assert_eq!(manifest.version, "1.2.3-beta+build");
    }

    #[test]
    fn growing_last_resource_section_updates_loader_image_bounds() {
        let stub = setup_fixture();
        let mut output = Vec::new();
        libsui::PortableExecutable::from(&stub)
            .unwrap()
            .write_resource("LARGE_PAYLOAD", vec![1; 64 * 1024])
            .unwrap()
            .build(&mut output)
            .unwrap();
        let image = parse_pe(&output).unwrap();
        let directory = image
            .data_directory(editpe::DataDirectoryType::ResourceTable)
            .unwrap();
        let section = image
            .section_header_for_data_directory(editpe::DataDirectoryType::ResourceTable)
            .unwrap();
        assert!(section.virtual_size >= directory.size);
        let size = image.windows_header().size_of_image();
        assert_eq!(size % image.windows_header().section_alignment(), 0);
        assert!(
            size >= section.virtual_address + section.virtual_size.max(section.size_of_raw_data)
        );
    }

    #[test]
    fn original_icon_and_group_payloads_survive_expansion_and_bundling() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = bundle_options(dir.path());
        options.setup_exe = setup_fixture_with_icon();
        let original = parse_pe(&options.setup_exe)
            .unwrap()
            .resource_directory()
            .unwrap()
            .clone();
        assert_eq!(original.get_icon().unwrap().unwrap(), ICON_PNG);
        let mut expanded = Vec::new();
        libsui::PortableExecutable::from(&options.setup_exe)
            .unwrap()
            .write_resource("LARGE", vec![0; 64 * 1024])
            .unwrap()
            .build(&mut expanded)
            .unwrap();
        let image = parse_pe(&expanded).unwrap();
        for kind in [3, 14] {
            assert_eq!(
                image
                    .resource_directory()
                    .unwrap()
                    .root()
                    .get(editpe::ResourceEntryName::ID(kind)),
                original.root().get(editpe::ResourceEntryName::ID(kind))
            );
        }
        let bundled = fs::read(bundle(options).unwrap().path).unwrap();
        let image = parse_pe(&bundled).unwrap();
        for kind in [3, 14] {
            assert_eq!(
                image
                    .resource_directory()
                    .unwrap()
                    .root()
                    .get(editpe::ResourceEntryName::ID(kind)),
                original.root().get(editpe::ResourceEntryName::ID(kind))
            );
        }
        let stub = setup_fixture_with_icon();
        let mut overridden = Vec::new();
        libsui::PortableExecutable::from(&stub)
            .unwrap()
            .set_icon(ICON_PNG)
            .unwrap()
            .build(&mut overridden)
            .unwrap();
        let image = parse_pe(&overridden).unwrap();
        let editpe::ResourceEntry::Table(groups) = image
            .resource_directory()
            .unwrap()
            .root()
            .get(editpe::ResourceEntryName::ID(14))
            .unwrap()
        else {
            panic!("Icon groups are not a table")
        };
        assert!(groups.get(editpe::ResourceEntryName::ID(1)).is_none());
        assert!(groups
            .get(editpe::ResourceEntryName::from_string("MAINICON"))
            .is_some());
    }

    #[test]
    fn stale_stub_certificate_is_removed_and_malformed_certificates_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = bundle_options(dir.path());
        options.setup_exe = setup_fixture_with_certificate();
        // The fixture is structurally certificate-bearing; trust is intentionally
        // outside this portable check and belongs to native signature verification.
        validate_certificate_table(&options.setup_exe).unwrap();
        let output = bundle(options).unwrap();
        assert!(validate_certificate_table(&fs::read(output.path).unwrap()).is_err());
        let mut bad = setup_fixture_with_certificate();
        let certificate = bad.len() - 16;
        put32(&mut bad, certificate, 100);
        assert!(validate_certificate_table(&bad).is_err());
        put32(&mut bad, certificate, 16);
        put16(&mut bad, certificate + 6, 1);
        assert!(validate_certificate_table(&bad).is_err());
    }

    #[test]
    fn pe_architecture_and_data_directory_bounds_must_agree() {
        let mut app = minimal_pe();
        put16(&mut app, 0x98, 0x10b);
        assert!(validate_application(&app).is_err());
        let mut app = minimal_pe();
        put32(&mut app, 0x98 + 108, 17);
        assert!(validate_application(&app).is_err());
        let app = minimal_pe();
        for end in 0..512 {
            assert!(validate_pe(&app[..end], false).is_err());
        }
    }

    #[test]
    fn rejects_dangerous_options_before_publishing() {
        let dir = tempfile::tempdir().unwrap();
        for id in ["", ".", "..", "../outside", r"C:\outside"] {
            let mut options = bundle_options(dir.path());
            options.identifier = id.into();
            assert!(bundle(options).is_err(), "accepted {id:?}");
        }
        for name in ["../outside", "NUL", "trailing."] {
            let mut options = bundle_options(dir.path());
            options.name = name.into();
            assert!(bundle(options).is_err(), "accepted {name:?}");
        }
        let app = dir.path().join("files");
        fs::create_dir_all(&app).unwrap();
        fs::write(dir.path().join("outside.exe"), minimal_pe()).unwrap();
        for main in ["", ".", "../outside.exe"] {
            let mut options = bundle_options(dir.path());
            options.app = app.clone();
            options.main_exe = Some(main.into());
            assert!(bundle(options).is_err(), "accepted {main:?}");
        }
        assert!(!dir.path().join("out").exists());
    }

    #[test]
    fn signer_preserves_windows_paths_and_replaces_placeholders_in_place() {
        let signer = SigningCommand::parse_legacy(
            r#""C:\Program Files\sign.exe" --key C:\certs\test.pfx --input="%1" --quiet"#,
        )
        .unwrap();
        assert_eq!(signer.program, PathBuf::from(r"C:\Program Files\sign.exe"));
        assert_eq!(
            signer.arguments(Path::new("my app.exe")).unwrap(),
            [
                "--key",
                r"C:\certs\test.pfx",
                "--input=my app.exe",
                "--quiet"
            ]
        );
        assert!(SigningCommand::parse_legacy(r#""unclosed"#).is_err());
        assert!(SigningCommand::parse_legacy("").is_err());
    }

    #[test]
    fn semantic_version_preserves_patch_and_rejects_overflow() {
        let v = parse_version_u32("1.2.3-beta+9").unwrap();
        assert_eq!(v.major, (1 << 16) | 2);
        assert_eq!(v.minor, 3 << 16);
        assert!(parse_version_u32("65536.2.3").is_err());
    }

    #[test]
    fn directory_archive_is_deterministic_and_rejects_case_collisions() {
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        for (root, names) in [
            (one.path(), ["b.txt", "a.exe"]),
            (two.path(), ["a.exe", "b.txt"]),
        ] {
            for name in names {
                fs::write(root.join(name), name.as_bytes()).unwrap();
            }
        }
        assert_eq!(
            create_tar_from_directory(one.path()).unwrap(),
            create_tar_from_directory(two.path()).unwrap()
        );
        #[cfg(not(windows))]
        {
            fs::write(one.path().join("A.EXE"), b"duplicate").unwrap();
            // Default macOS filesystems merge the two names. Linux CI exercises
            // the collision when the source filesystem permits distinct entries.
            let duplicates = fs::read_dir(one.path())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case("a.exe")
                })
                .count();
            if duplicates == 2 {
                assert!(create_tar_from_directory(one.path()).is_err());
            }
        }
    }

    #[test]
    fn archive_contract_counts_gnu_headers_and_implicit_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source");
        fs::write(&path, b"data").unwrap();
        let tar = create_tar_from_file(&path, &format!("{}.txt", "a".repeat(120))).unwrap();
        assert!(validate_archive_contract(&tar, 1).is_err());
        assert_eq!(validate_archive_contract(&tar, 2).unwrap(), 4);
        let tar = create_tar_from_file(&path, "one/two/app.exe").unwrap();
        assert!(validate_archive_contract(&tar, 2).is_err());
        assert_eq!(validate_archive_contract(&tar, 3).unwrap(), 4);
    }

    #[test]
    fn deep_application_directory_is_rejected_before_publication() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = bundle_options(dir.path());
        options.app = dir.path().into();
        options.main_exe = Some("app.exe".into());
        let mut nested = dir.path().to_path_buf();
        for _ in 0..=crate::MAX_ARCHIVE_DEPTH {
            nested = nested.join("a");
        }
        fs::create_dir_all(&nested).unwrap();
        assert!(matches!(bundle(options), Err(BundleError::Validation(_))));
        assert!(!dir.path().join("out").exists());
    }

    #[cfg(unix)]
    #[test]
    fn archive_rejects_symlink_escape_and_sign_failure_preserves_old_output() {
        let dir = tempfile::tempdir().unwrap();
        let files = dir.path().join("files");
        fs::create_dir_all(&files).unwrap();
        fs::write(dir.path().join("outside"), b"private").unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside"), files.join("escape")).unwrap();
        assert!(create_tar_from_directory(&files).is_err());
        let mut options = bundle_options(dir.path());
        fs::create_dir_all(&options.output_dir).unwrap();
        let previous = options.output_dir.join("test-app-setup.exe");
        fs::write(&previous, b"previous release").unwrap();
        options.sign_command = Some(SigningCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "exit 1".into()],
        });
        assert!(matches!(bundle(options), Err(BundleError::Sign(_))));
        assert_eq!(fs::read(&previous).unwrap(), b"previous release");
        // A signer that exits successfully without signing is also a failure.
        let mut options = bundle_options(dir.path());
        options.setup_exe = setup_fixture_with_certificate();
        options.sign_command = Some(SigningCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), "exit 0".into()],
        });
        assert!(matches!(bundle(options), Err(BundleError::Sign(_))));
        assert_eq!(fs::read(previous).unwrap(), b"previous release");
    }
}
