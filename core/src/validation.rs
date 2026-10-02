//! Validate Windows paths consistently on build hosts and end-user machines.
use crate::{InstallMetadata, SetupManifest, INSTALL_METADATA_FILENAME, MAX_METADATA_SIZE};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

// Shared package limits keep build-time acceptance and runtime extraction aligned.
pub const MAX_ARCHIVE_ENTRIES: usize = 100_000;
pub const MAX_ARCHIVE_DEPTH: usize = 64;
pub const MAX_ARCHIVE_PATH_BYTES: u64 = 32_768;
pub const MAX_UNPACKED_SIZE: u64 = 32 * 1024 * 1024 * 1024;
pub const MAX_ARCHIVE_SIZE: u64 = MAX_UNPACKED_SIZE + 1024 * 1024 * 1024;
pub const MAX_ZSTD_WINDOW_SIZE: u64 = 128 * 1024 * 1024;

/// Apply limits after converting an archive path to relative forward-slash form.
pub fn validate_archive_path_limits(path: &str) -> Result<(), String> {
    if path.len() as u64 > MAX_ARCHIVE_PATH_BYTES {
        return Err("An archive path exceeds its supported length.".into());
    }
    if path.split('/').count() > MAX_ARCHIVE_DEPTH {
        return Err("An archive path exceeds the maximum supported depth (64).".into());
    }
    Ok(())
}

/// Validate frame limits before a decoder allocates its window.
pub fn validate_archive_zstd_header(data: &[u8]) -> Result<(), String> {
    if data.get(..4) != Some(&[0x28, 0xb5, 0x2f, 0xfd]) {
        return Err("The application payload is not a standard zstd frame.".into());
    }
    let descriptor = *data.get(4).ok_or("Truncated zstd frame header.")?;
    if descriptor & 0x1b != 0 {
        return Err("Unsupported zstd frame flags or dictionary.".into());
    }
    let single = descriptor & 0x20 != 0;
    let mut offset = 5usize;
    if !single {
        let window_descriptor = *data
            .get(offset)
            .ok_or("Truncated zstd window descriptor.")?;
        offset += 1;
        let base = 1u64
            .checked_shl(10 + (window_descriptor >> 3) as u32)
            .ok_or("Invalid zstd window.")?;
        let window = base + (base / 8) * (window_descriptor & 7) as u64;
        if window > MAX_ZSTD_WINDOW_SIZE {
            return Err("The zstd window exceeds the 128 MiB memory limit.".into());
        }
    }
    let size_bytes = match descriptor >> 6 {
        0 if single => 1,
        0 => 0,
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let bytes = data
        .get(offset..offset + size_bytes)
        .ok_or("Truncated zstd content size.")?;
    let mut content = [0u8; 8];
    content[..size_bytes].copy_from_slice(bytes);
    let mut content_size = u64::from_le_bytes(content);
    if size_bytes == 2 {
        content_size += 256;
    }
    if content_size > MAX_ARCHIVE_SIZE || (single && content_size > MAX_ZSTD_WINDOW_SIZE) {
        return Err("The zstd frame exceeds the supported size or memory limit.".into());
    }
    Ok(())
}

pub fn validate_windows_filename(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.encode_utf16().count() > 200
        || value.ends_with(['.', ' '])
        || value == "."
        || value == ".."
        || value
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err(format!("Invalid Windows filename: {value:?}"));
    }
    let stem = value.split('.').next().unwrap_or_default().to_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        });
    if device {
        return Err(format!("Reserved Windows device filename: {value:?}"));
    }
    Ok(())
}

pub fn validate_identifier(identifier: &str) -> Result<(), String> {
    validate_windows_filename(identifier)?;
    if identifier.len() > 128
        || !identifier.as_bytes()[0].is_ascii_alphanumeric()
        || !identifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        return Err("Application identifier must be a nonempty ASCII component containing letters, digits, dots, underscores, or hyphens".into());
    }
    Ok(())
}

pub fn validate_relative_executable(path: &str) -> Result<(), String> {
    if path.len() > 1024 || path.is_empty() {
        return Err("Main executable path must be nonempty and relative".into());
    }
    validate_archive_path_limits(&path.replace('\\', "/"))?;
    for component in path.split(['/', '\\']) {
        validate_windows_filename(component)?;
    }
    if !path.to_ascii_lowercase().ends_with(".exe") {
        return Err("Main executable must have an .exe extension".into());
    }
    Ok(())
}

pub fn executable_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    validate_relative_executable(relative)?;
    Ok(relative
        .split(['/', '\\'])
        .fold(root.to_path_buf(), |path, part| path.join(part)))
}

pub fn validate_manifest(manifest: &SetupManifest) -> Result<(), String> {
    validate_identifier(&manifest.identifier)?;
    validate_windows_filename(&manifest.name)?;
    validate_windows_filename(&manifest.title)?;
    validate_relative_executable(&manifest.application)?;
    if manifest.unpacked_size > MAX_UNPACKED_SIZE {
        return Err("Installed application exceeds the supported 32 GiB limit.".into());
    }
    semver::Version::parse(&manifest.version)
        .map_err(|e| format!("Invalid application version: {e}"))?;
    if manifest.publisher.len() > 4096 || manifest.publisher.chars().any(|c| c.is_control()) {
        return Err("Publisher contains control characters or exceeds its size limit".into());
    }
    if !manifest.bundle_sha256.is_empty()
        && (manifest.bundle_sha256.len() != 64
            || !manifest
                .bundle_sha256
                .bytes()
                .all(|c| c.is_ascii_hexdigit()))
    {
        return Err("Bundle digest must be a SHA-256 hexadecimal string".into());
    }
    Ok(())
}

pub fn reject_reparse_point(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Cannot inspect {}: {error}", path.display())),
    };
    let reparse = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        reparse || metadata.file_attributes() & 0x400 != 0
    };
    if reparse {
        return Err(format!(
            "Refusing a symbolic link or reparse point: {}",
            path.display()
        ));
    }
    Ok(())
}

/// Programs must already exist. Return only a dedicated immediate child.
pub fn validate_install_root(programs: &Path, identifier: &str) -> Result<PathBuf, String> {
    validate_identifier(identifier)?;
    reject_reparse_point(programs)?;
    let parent = fs::canonicalize(programs)
        .map_err(|e| format!("Cannot resolve Programs directory: {e}"))?;
    let root = programs.join(identifier);
    reject_reparse_point(&root)?;
    if root.exists() {
        let actual =
            fs::canonicalize(&root).map_err(|e| format!("Cannot resolve install root: {e}"))?;
        if actual.parent() != Some(parent.as_path()) || !actual.is_dir() {
            return Err("Installation root must be an immediate directory inside Programs".into());
        }
    }
    Ok(root)
}

pub fn read_install_metadata(root: &Path) -> Result<InstallMetadata, String> {
    let path = root.join(INSTALL_METADATA_FILENAME);
    reject_reparse_point(&path)?;
    let mut data = Vec::new();
    fs::File::open(&path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?
        .take(MAX_METADATA_SIZE as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| format!("Cannot read install metadata: {e}"))?;
    if data.len() > MAX_METADATA_SIZE {
        return Err("Install metadata exceeds the size limit".into());
    }
    let metadata: InstallMetadata =
        serde_json::from_slice(&data).map_err(|e| format!("Invalid install metadata: {e}"))?;
    metadata.validate()?;
    Ok(metadata)
}

/// Locate ownership metadata for a root-level or nested executable.
pub fn discover_installation(
    exe: &Path,
    programs: &Path,
) -> Result<(PathBuf, InstallMetadata), String> {
    reject_reparse_point(programs)?;
    let exe = fs::canonicalize(exe).map_err(|e| format!("Cannot resolve executable: {e}"))?;
    let programs =
        fs::canonicalize(programs).map_err(|e| format!("Cannot resolve Programs: {e}"))?;
    let root = exe
        .ancestors()
        .skip(1)
        .find(|path| path.parent() == Some(programs.as_path()))
        .ok_or("Executable is not within a managed Programs installation")?;
    let metadata = read_install_metadata(root)?;
    let expected_root = validate_install_root(&programs, &metadata.app_id)?;
    if fs::canonicalize(expected_root).map_err(|e| e.to_string())? != root {
        return Err("Install metadata does not own this installation root".into());
    }
    let expected_exe = executable_path(root, &metadata.app_exe)?;
    reject_reparse_point(&expected_exe)?;
    if fs::canonicalize(expected_exe).map_err(|e| e.to_string())? != exe {
        return Err("Install metadata does not identify the running executable".into());
    }
    Ok((root.to_path_buf(), metadata))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_limits_reject_deep_paths_and_oversized_windows() {
        let accepted = vec!["a"; MAX_ARCHIVE_DEPTH].join("/");
        validate_archive_path_limits(&accepted).unwrap();
        assert!(validate_archive_path_limits(&format!("{accepted}/b")).is_err());
        assert!(
            validate_archive_path_limits(&"a".repeat(MAX_ARCHIVE_PATH_BYTES as usize + 1)).is_err()
        );
        assert!(validate_archive_zstd_header(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0xff]).is_err());
    }
    #[test]
    fn windows_names_and_paths_are_checked_on_every_host() {
        for invalid in [
            "", ".", "..", "CON", "NUL.exe", "COM1.txt", "LPT³", "a.", "a ", "a/b", "a\\b",
            "C:foo", "a\0b",
        ] {
            assert!(validate_windows_filename(invalid).is_err(), "{invalid:?}");
        }
        for invalid in ["", "..", "../outside", "C:\\Outside", "/outside", "a b"] {
            assert!(validate_identifier(invalid).is_err(), "{invalid:?}");
        }
        for invalid in [
            "",
            ".",
            "../app.exe",
            "bin/../app.exe",
            "C:\\app.exe",
            "//host/app.exe",
            "bin//app.exe",
            "bin/CON.exe",
            "readme.txt",
        ] {
            assert!(
                validate_relative_executable(invalid).is_err(),
                "{invalid:?}"
            );
        }
        validate_windows_filename("日本語 App").unwrap();
        validate_relative_executable("bin\\My App.exe").unwrap();
    }
    #[test]
    fn nested_executable_discovery_checks_ownership() {
        let temporary = tempfile::tempdir().unwrap();
        let programs = temporary.path().join("Programs");
        let root = programs.join("com.example.app");
        fs::create_dir_all(root.join("bin")).unwrap();
        let exe = root.join("bin/app.exe");
        fs::write(&exe, b"test").unwrap();
        let mut metadata = InstallMetadata {
            format_version: 1,
            app_title: "Test App".into(),
            app_id: "com.example.app".into(),
            app_exe: "bin/app.exe".into(),
            version: "1.0.0".into(),
            desktop_shortcut: false,
            shortcut_title: None,
        };
        fs::write(
            root.join(INSTALL_METADATA_FILENAME),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        assert_eq!(
            discover_installation(&exe, &programs).unwrap().0,
            fs::canonicalize(&root).unwrap()
        );
        metadata.app_id = "com.example.other".into();
        fs::write(
            root.join(INSTALL_METADATA_FILENAME),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        assert!(discover_installation(&exe, &programs).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn installation_roots_cannot_be_links() {
        let temporary = tempfile::tempdir().unwrap();
        let programs = temporary.path().join("Programs");
        let outside = temporary.path().join("outside");
        fs::create_dir_all(&programs).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, programs.join("com.example.app")).unwrap();
        assert!(validate_install_root(&programs, "com.example.app").is_err());
    }
}
