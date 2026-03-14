use bytesize::ByteSize;
use dirs_next::cache_dir;
use reqwest::blocking::get;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const WEBVIEW2_EVERGREEN_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
pub const WEBVIEW2_EVERGREEN_EXE: &str = "MicrosoftEdgeWebview2Setup.exe";

/// Download the WebView2 evergreen bootstrapper, using a local cache.
pub fn download_webview2_evergreen() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    download_webview2_evergreen_impl(WEBVIEW2_EVERGREEN_URL)
}

fn download_webview2_evergreen_impl(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let cache_dir = std::env::var("CACHE_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| cache_dir().map(|d| d.join("webview2")))
        .ok_or("Failed to determine cache directory")?;

    let webview2_path = cache_dir.join(WEBVIEW2_EVERGREEN_EXE);

    println!("  Downloading WebView2 Evergreen: {}", url);

    if !webview2_path.exists() {
        fs::create_dir_all(&cache_dir)?;

        let response = get(url)?;
        let bytes = response.bytes()?;
        let mut file = fs::File::create(&webview2_path)?;
        file.write_all(&bytes)?;
    }

    let webview2_data = fs::read(&webview2_path)?;
    println!(
        "  Loaded WebView2 Evergreen: {} ({})",
        WEBVIEW2_EVERGREEN_EXE,
        ByteSize(webview2_data.len() as u64)
    );

    Ok(webview2_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_download_webview2_evergreen_download() {
        let mut server = Server::new();

        let mock = server
            .mock("GET", "/webview2")
            .with_status(200)
            .with_header("content-type", "application/octet-stream")
            .with_body(b"mock webview2 installer")
            .create();

        let mock_url = format!("{}/webview2", server.url());
        let cache_dir = tempdir().expect("Failed to create temp dir");
        let webview2_path = cache_dir.path().join(WEBVIEW2_EVERGREEN_EXE);

        assert!(!webview2_path.exists());

        std::env::set_var("CACHE_DIR", cache_dir.path());

        let bytes = download_webview2_evergreen_impl(&mock_url).unwrap();
        assert_eq!(bytes, b"mock webview2 installer");

        assert!(webview2_path.exists());
        mock.assert();
    }

    #[test]
    fn test_download_webview2_evergreen_cached() {
        let cache_dir = tempdir().expect("Failed to create temp dir");
        let webview2_path = cache_dir.path().join(WEBVIEW2_EVERGREEN_EXE);

        {
            let mut file = fs::File::create(&webview2_path).expect("Failed to create file");
            file.write_all(b"mock cached installer")
                .expect("Failed to write to file");
        }

        assert!(webview2_path.exists());

        let mut server = Server::new();

        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/octet-stream")
            .with_body(b"mock webview2 installer")
            .expect(0)
            .create();

        let mock_url = server.url();
        std::env::set_var("CACHE_DIR", cache_dir.path());

        let bytes = download_webview2_evergreen_impl(&mock_url).unwrap();
        assert_eq!(bytes, b"mock cached installer");

        mock.assert();
    }
}
