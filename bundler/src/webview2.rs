use bytesize::ByteSize;
use dirs_next::cache_dir;
use reqwest::blocking::Client;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

const WEBVIEW2_EVERGREEN_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
pub const WEBVIEW2_EVERGREEN_EXE: &str = "MicrosoftEdgeWebview2Setup.exe";
const MAX_BOOTSTRAPPER_SIZE: u64 = 32 * 1024 * 1024;

/// Download the Evergreen bootstrapper. The explicit cache argument keeps
/// concurrent callers and tests independent of process-global environment state.
pub fn download_webview2_evergreen(
    cache: Option<&Path>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let default_cache = cache_dir().map(|d| d.join("twi").join("webview2"));
    let cache = cache
        .or(default_cache.as_deref())
        .ok_or("Cannot determine WebView2 cache directory")?;
    download_webview2_evergreen_impl(WEBVIEW2_EVERGREEN_URL, cache, Duration::from_secs(120))
}

fn download_webview2_evergreen_impl(
    url: &str,
    cache: &Path,
    timeout: Duration,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    fs::create_dir_all(cache)?;
    // OS-backed locks are released even when a process crashes. The lock file is
    // deliberately retained so two processes cannot lock different file objects.
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(cache.join("bootstrapper.lock"))?;
    lock.lock()?;
    let path = cache.join(WEBVIEW2_EVERGREEN_EXE);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("WebView2 cache entry is not a regular file".into());
        }
        if metadata.len() <= MAX_BOOTSTRAPPER_SIZE {
            let data = fs::read(&path)?;
            if twi_core::bundle::validate_pe(&data, false).is_ok() {
                println!(
                    "  Loaded WebView2 Evergreen: {} ({})",
                    WEBVIEW2_EVERGREEN_EXE,
                    ByteSize(data.len() as u64)
                );
                return Ok(data);
            }
        }
        // A corrupt prior download is never treated as a valid installer.
        fs::remove_file(&path)?;
    }
    println!("  Downloading WebView2 Evergreen: {url}");
    let client = Client::builder()
        .https_only(url.starts_with("https://"))
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .build()?;
    let response = client.get(url).send()?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BOOTSTRAPPER_SIZE)
    {
        return Err("WebView2 bootstrapper exceeds the size limit".into());
    }
    let mut data = Vec::new();
    response
        .take(MAX_BOOTSTRAPPER_SIZE + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_BOOTSTRAPPER_SIZE {
        return Err("WebView2 bootstrapper exceeds the size limit".into());
    }
    twi_core::bundle::validate_pe(&data, false)?;
    let mut staged = tempfile::NamedTempFile::new_in(cache)?;
    staged.write_all(&data)?;
    staged.as_file().sync_all()?;
    staged.persist(&path)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use tempfile::tempdir;

    fn executable() -> Vec<u8> {
        let mut data = vec![0u8; 1024];
        data[..2].copy_from_slice(b"MZ");
        data[60..64].copy_from_slice(&128u32.to_le_bytes());
        data[128..132].copy_from_slice(b"PE\0\0");
        data[132..134].copy_from_slice(&0x8664u16.to_le_bytes());
        data[134..136].copy_from_slice(&1u16.to_le_bytes());
        data[148..150].copy_from_slice(&240u16.to_le_bytes());
        data[150..152].copy_from_slice(&0x22u16.to_le_bytes());
        data[152..154].copy_from_slice(&0x20bu16.to_le_bytes());
        data[392 + 16..392 + 20].copy_from_slice(&512u32.to_le_bytes());
        data[392 + 20..392 + 24].copy_from_slice(&512u32.to_le_bytes());
        data
    }

    #[test]
    fn download_and_cache_a_valid_executable() {
        let mut server = Server::new();
        let bytes = executable();
        let mock = server
            .mock("GET", "/webview2")
            .with_status(200)
            .with_body(&bytes)
            .expect(1)
            .create();
        let cache = tempdir().unwrap();
        let url = format!("{}/webview2", server.url());
        assert_eq!(
            download_webview2_evergreen_impl(&url, cache.path(), Duration::from_secs(5)).unwrap(),
            bytes
        );
        assert_eq!(
            download_webview2_evergreen_impl(&url, cache.path(), Duration::from_secs(5)).unwrap(),
            bytes
        );
        mock.assert();
    }

    #[test]
    fn reject_http_error_and_html_without_publishing_a_cache() {
        let mut server = Server::new();
        let cache = tempdir().unwrap();
        let url = format!("{}/webview2", server.url());
        let error = server
            .mock("GET", "/webview2")
            .with_status(503)
            .with_body("temporary error")
            .create();
        assert!(
            download_webview2_evergreen_impl(&url, cache.path(), Duration::from_secs(5)).is_err()
        );
        error.assert();
        error.remove();
        let html = server
            .mock("GET", "/webview2")
            .with_status(200)
            .with_body("<html>error</html>")
            .create();
        assert!(
            download_webview2_evergreen_impl(&url, cache.path(), Duration::from_secs(5)).is_err()
        );
        html.assert();
        assert!(!cache.path().join(WEBVIEW2_EVERGREEN_EXE).exists());
    }

    #[test]
    fn concurrent_downloads_repair_an_empty_cache_once() {
        let mut server = Server::new();
        let bytes = executable();
        let mock = server
            .mock("GET", "/webview2")
            .with_status(200)
            .with_body(&bytes)
            .expect(1)
            .create();
        let cache = tempdir().unwrap();
        fs::write(cache.path().join(WEBVIEW2_EVERGREEN_EXE), []).unwrap();
        let url = format!("{}/webview2", server.url());
        std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                download_webview2_evergreen_impl(&url, cache.path(), Duration::from_secs(5))
                    .unwrap()
            });
            let two = scope.spawn(|| {
                download_webview2_evergreen_impl(&url, cache.path(), Duration::from_secs(5))
                    .unwrap()
            });
            assert_eq!(one.join().unwrap(), bytes);
            assert_eq!(two.join().unwrap(), bytes);
        });
        mock.assert();
    }
}
