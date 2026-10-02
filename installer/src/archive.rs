//! Bounded streaming validation/extraction of the bundler's GNU tar + zstd format.
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use twi_core::{MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_PATH_BYTES, MAX_ARCHIVE_SIZE, MAX_UNPACKED_SIZE};

#[derive(Debug, PartialEq)]
pub struct ArchiveInfo {
    pub unpacked_size: u64,
    pub allocation_size: u64,
}

pub fn verify_digest(data: &[u8], expected: &str) -> Result<(), String> {
    if expected.is_empty() {
        return Ok(());
    } // Legacy packages have no checksum.
    let digest = Sha256::digest(data);
    let actual: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(
            "The application payload checksum does not match. Download the installer again.".into(),
        );
    }
    Ok(())
}

pub fn inspect(
    data: &[u8],
    expected_size: u64,
    allocation_unit: u64,
) -> Result<ArchiveInfo, String> {
    process_archive(data, expected_size, allocation_unit, None)
}

pub fn extract(data: &[u8], expected_size: u64, destination: &Path) -> Result<(), String> {
    process_archive(data, expected_size, 4096, Some(destination)).map(|_| ())
}

fn process_archive(
    data: &[u8],
    expected_size: u64,
    allocation_unit: u64,
    destination: Option<&Path>,
) -> Result<ArchiveInfo, String> {
    twi_core::validate_archive_zstd_header(data)?;
    if allocation_unit == 0 || allocation_unit > 32 * 1024 * 1024 {
        return Err("Unsupported filesystem allocation unit.".into());
    }
    if expected_size > MAX_UNPACKED_SIZE {
        return Err("The package exceeds the maximum supported installed size (32 GiB).".into());
    }
    let mut input = Cursor::new(data);
    let decoder = ruzstd::streaming_decoder::StreamingDecoder::new(&mut input)
        .map_err(|error| format!("Cannot initialize the application decompressor: {error}"))?;
    let mut reader = BoundedReader {
        inner: decoder,
        consumed: 0,
        limit: MAX_ARCHIVE_SIZE,
    };
    let mut archive = tar::Archive::new(&mut reader);
    let mut paths = HashSet::new();
    let mut files = HashSet::new();
    let mut directories = HashSet::new();
    let mut long_name = None;
    let mut entries_count = 0usize;
    let mut total = 0u64;
    let mut allocation = 0u64;
    // Raw mode prevents tar from allocating unbounded buffers for long-name/PAX
    // entries. The bundler only emits GNU headers; links/PAX/sparse files are rejected.
    for entry in archive
        .entries()
        .map_err(|error| error.to_string())?
        .raw(true)
    {
        let mut entry = entry.map_err(|error| format!("Invalid application archive: {error}"))?;
        entries_count += 1;
        if entries_count > MAX_ARCHIVE_ENTRIES {
            return Err("The application archive has too many entries.".into());
        }
        let kind = entry.header().entry_type();
        let size = entry.header().size().map_err(|error| error.to_string())?;
        if kind.is_gnu_longname() {
            if long_name.is_some() || size > MAX_ARCHIVE_PATH_BYTES {
                return Err("Invalid or oversized archive long-name entry.".into());
            }
            let mut bytes = Vec::new();
            entry
                .take(MAX_ARCHIVE_PATH_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.last() == Some(&0) {
                bytes.pop();
            }
            long_name =
                Some(String::from_utf8(bytes).map_err(|_| "An archive path is not valid UTF-8.")?);
            continue;
        }
        if !kind.is_file() && !kind.is_dir() {
            return Err(
                "The application archive contains a link, sparse file, or unsupported entry."
                    .into(),
            );
        }
        let name = match long_name.take() {
            Some(name) => name,
            None => entry
                .path()
                .map_err(|error| error.to_string())?
                .to_str()
                .ok_or_else(|| "An archive path is not valid UTF-8.".to_string())?
                .to_string(),
        };
        let relative = validate_archive_path(&name, kind.is_dir())?;
        if relative.as_os_str().is_empty() {
            if size != 0 {
                return Err("The archive root directory contains data.".into());
            }
            continue;
        }
        let identity = path_identity(&relative);
        if !paths.insert(identity) {
            return Err(format!(
                "The archive contains a duplicate Windows path: {name}"
            ));
        }
        for parent in relative
            .ancestors()
            .skip(1)
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let parent = path_identity(parent);
            if files.contains(&parent) {
                return Err("An archive file is also used as a directory.".into());
            }
            if directories.insert(parent) {
                allocation = allocation
                    .checked_add(allocation_unit)
                    .ok_or("Archive size overflow.")?;
            }
        }
        if kind.is_dir() {
            if size != 0 {
                return Err("An archive directory contains data.".into());
            }
            if files.contains(&identity) {
                return Err("An archive file is also used as a directory.".into());
            }
            if directories.insert(identity) {
                allocation = allocation
                    .checked_add(allocation_unit)
                    .ok_or("Archive size overflow.")?;
            }
            if directories.len() + files.len() > MAX_ARCHIVE_ENTRIES {
                return Err("The archive expands to too many files and directories.".into());
            }
            if let Some(destination) = destination {
                fs::create_dir_all(destination.join(&relative))
                    .map_err(|error| error.to_string())?;
            }
            continue;
        }
        if directories.contains(&identity) {
            return Err("An archive directory is also used as a file.".into());
        }
        files.insert(identity);
        if directories.len() + files.len() > MAX_ARCHIVE_ENTRIES {
            return Err("The archive expands to too many files and directories.".into());
        }
        total = total.checked_add(size).ok_or("Archive size overflow.")?;
        if total > MAX_UNPACKED_SIZE || (expected_size != 0 && total > expected_size) {
            return Err("The archive expands beyond its declared or supported size.".into());
        }
        allocation = allocation
            .checked_add(
                size.checked_add(allocation_unit - 1)
                    .ok_or("Archive size overflow.")?
                    / allocation_unit
                    * allocation_unit,
            )
            .ok_or("Archive allocation size overflow.")?;
        if let Some(destination) = destination {
            let target = destination.join(&relative);
            fs::create_dir_all(target.parent().ok_or("An archive file has no parent.")?)
                .map_err(|error| error.to_string())?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|error| format!("Cannot create {}: {error}", target.display()))?;
            let written = io::copy(&mut entry, &mut file).map_err(|error| error.to_string())?;
            if written != size {
                return Err("An archive file is truncated.".into());
            }
            file.sync_all().map_err(|error| error.to_string())?;
        } else {
            let read = io::copy(&mut entry, &mut io::sink()).map_err(|error| error.to_string())?;
            if read != size {
                return Err("An archive file is truncated.".into());
            }
        }
    }
    if long_name.is_some() {
        return Err("The archive ends with an unused long-name entry.".into());
    }
    if expected_size != 0 && total != expected_size {
        return Err("The archive's installed size does not match its manifest.".into());
    }
    // Force verification of the zstd frame checksum even if tar stops at its EOF blocks.
    io::copy(&mut reader, &mut io::sink())
        .map_err(|error| format!("Cannot finish decompressing the archive: {error}"))?;
    let declared_content = reader.inner.decoder.content_size();
    if declared_content != 0 && declared_content != reader.consumed {
        return Err("The zstd frame's decoded size does not match its header.".into());
    }
    if let Some(checksum) = reader.inner.decoder.get_checksum_from_data() {
        if reader.inner.decoder.get_calculated_checksum() != Some(checksum) {
            return Err("The zstd frame checksum does not match its decoded contents.".into());
        }
    }
    drop(reader);
    if input.position() != data.len() as u64 {
        return Err("The compressed application archive contains trailing data.".into());
    }
    Ok(ArchiveInfo {
        unpacked_size: total,
        allocation_size: allocation,
    })
}

fn validate_archive_path(input: &str, directory: bool) -> Result<PathBuf, String> {
    if input.len() as u64 > MAX_ARCHIVE_PATH_BYTES || input.contains('\\') {
        return Err(format!("Invalid Windows archive path: {input}"));
    }
    let mut input = input;
    while let Some(rest) = input.strip_prefix("./") {
        input = rest;
    }
    if directory {
        input = input.trim_end_matches('/');
    }
    if directory && (input.is_empty() || input == ".") {
        return Ok(PathBuf::new());
    }
    twi_core::validate_archive_path_limits(input)?;
    let mut result = PathBuf::new();
    for component in input.split('/') {
        twi_core::validate_windows_filename(component)?;
        if result.as_os_str().is_empty()
            && component.eq_ignore_ascii_case(twi_core::INSTALL_METADATA_FILENAME)
        {
            return Err(format!("Reserved Windows archive path: {input}"));
        }
        result.push(component);
    }
    if result.as_os_str().is_empty() {
        return Err("An archive file has an empty path.".into());
    }
    Ok(result)
}

fn path_identity(path: &Path) -> [u8; 32] {
    // Fixed-size identities bound memory even for 100,000 long paths.
    Sha256::digest(
        path.to_string_lossy()
            .replace('\\', "/")
            .to_lowercase()
            .as_bytes(),
    )
    .into()
}

struct BoundedReader<R> {
    inner: R,
    consumed: u64,
    limit: u64,
}
impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.consumed == self.limit {
            let mut byte = [0];
            if self.inner.read(&mut byte)? == 0 {
                return Ok(0);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Decompressed archive exceeds its size limit.",
            ));
        }
        let length = bytes.len().min((self.limit - self.consumed) as usize);
        let read = self.inner.read(&mut bytes[..length])?;
        self.consumed += read as u64;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Zstd raw-block frame, avoiding a C compression dependency in cross builds.
    fn compressed_tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        let tar = builder.into_inner().unwrap();
        assert!(tar.len() < 128 * 1024);
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0xa0];
        frame.extend_from_slice(&(tar.len() as u32).to_le_bytes());
        let block = ((tar.len() as u32) << 3) | 1;
        frame.extend_from_slice(&block.to_le_bytes()[..3]);
        frame.extend_from_slice(&tar);
        frame
    }

    #[test]
    fn streaming_roundtrip_reports_uncompressed_space() {
        let bytes = compressed_tar(&[("app.exe", b"image"), ("assets/config.json", b"{}")]);
        let info = inspect(&bytes, 7, 4096).unwrap();
        assert_eq!(info.unpacked_size, 7);
        assert_eq!(info.allocation_size, 12288);
        let dir = tempfile::tempdir().unwrap();
        extract(&bytes, 7, dir.path()).unwrap();
        assert_eq!(
            fs::read(dir.path().join("assets/config.json")).unwrap(),
            b"{}"
        );
    }

    #[test]
    fn wrong_size_and_checksum_are_rejected() {
        let bytes = compressed_tar(&[("app.exe", b"image")]);
        assert!(inspect(&bytes, 4, 4096).is_err());
        assert!(inspect(&bytes, 6, 4096).is_err());
        assert!(verify_digest(&bytes, &"0".repeat(64)).is_err());
        assert!(verify_digest(&bytes, &format!("{:x}", Sha256::digest(&bytes))).is_ok());
    }

    #[test]
    fn duplicate_case_insensitive_paths_are_rejected() {
        let bytes = compressed_tar(&[("app.exe", b"first"), ("APP.EXE", b"second")]);
        assert!(inspect(&bytes, 0, 4096).is_err());
    }

    #[test]
    fn windows_path_tricks_are_rejected_on_every_platform() {
        for path in [
            "../outside",
            "/outside",
            "C:/outside",
            "dir\\outside",
            "dir/file:stream",
            "NUL.txt",
            "dir/CON",
            "dir/file.",
            ".twi-meta.json",
        ] {
            assert!(validate_archive_path(path, false).is_err(), "{path}");
        }
        assert!(validate_archive_path("./bin/app.exe", false).is_ok());
    }

    #[test]
    fn oversized_window_rejected_before_decoder_allocation() {
        assert!(
            twi_core::validate_archive_zstd_header(&[0x28, 0xb5, 0x2f, 0xfd, 0, 0xff]).is_err()
        );
    }

    #[test]
    fn truncated_frames_fail_without_modifying_live_files() {
        let mut bytes = compressed_tar(&[("app.exe", b"image")]);
        bytes.truncate(bytes.len() - 1024);
        assert!(inspect(&bytes, 0, 4096).is_err());
    }

    #[test]
    fn gnu_long_paths_extract_with_bounded_metadata() {
        let name = format!("assets/{}/config.json", "a".repeat(120));
        let bytes = compressed_tar(&[(&name, b"{}")]);
        assert_eq!(inspect(&bytes, 2, 4096).unwrap().allocation_size, 12288);
        let directory = tempfile::tempdir().unwrap();
        extract(&bytes, 2, directory.path()).unwrap();
        assert_eq!(fs::read(directory.path().join(name)).unwrap(), b"{}");
    }

    #[test]
    fn actual_cluster_size_and_implicit_directories_are_counted() {
        let bytes = compressed_tar(&[("one/two/app.exe", b"x")]);
        assert_eq!(
            inspect(&bytes, 1, 65_536).unwrap().allocation_size,
            3 * 65_536
        );
        assert!(inspect(&bytes, 1, 0).is_err());
    }

    #[test]
    fn files_cannot_be_used_as_parent_directories() {
        let bytes = compressed_tar(&[("assets", b"file"), ("assets/config.json", b"{}")]);
        assert!(inspect(&bytes, 0, 4096).is_err());
    }

    #[test]
    fn zstd_content_size_is_enforced() {
        let mut bytes = compressed_tar(&[("app.exe", b"image")]);
        bytes[5..9].copy_from_slice(&1u32.to_le_bytes());
        assert!(inspect(&bytes, 5, 4096).is_err());
    }

    #[test]
    fn normalized_archive_paths_enforce_shared_depth_limit() {
        let boundary = format!(
            "./{}/app.exe",
            "a/".repeat(twi_core::MAX_ARCHIVE_DEPTH - 2)
                .trim_end_matches('/')
        );
        assert!(validate_archive_path(&boundary, false).is_ok());
        let excessive = format!("./{}app.exe", "a/".repeat(twi_core::MAX_ARCHIVE_DEPTH));
        assert!(validate_archive_path(&excessive, false).is_err());
        let bytes = compressed_tar(&[(&excessive, b"x")]);
        assert!(inspect(&bytes, 1, 4096).is_err());
        assert!(validate_archive_path("./././", true)
            .unwrap()
            .as_os_str()
            .is_empty());
    }
}
