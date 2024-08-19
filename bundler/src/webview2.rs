use bytesize::ByteSize;
use dirs_next::cache_dir;
use reqwest::blocking::get;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const WEBVIEW2_EVERGREEN_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
pub const WEBVIEW2_EVERGREEN_EXE: &str = "MicrosoftEdgeWebview2Setup.exe";

/// Download the WebView2 evergreen bootstrapper
pub fn download_webview2_evergreen() -> Vec<u8> {
    download_webview2_evergreen_impl(WEBVIEW2_EVERGREEN_URL)
}

/// Download the WebView2 evergreen bootstrapper
pub fn download_webview2_evergreen_impl(url: &str) -> Vec<u8> {
    // Get the cache directory
    let cache_dir = std::env::var("CACHE_DIR")
        .ok()
        .map(PathBuf::from)
        .or_else(|| cache_dir().map(|d| d.join("webview2")))
        .expect("Failed to get cache directory");

    let webview2_path = cache_dir.join(WEBVIEW2_EVERGREEN_EXE);

    println!("  Downloading WebView2 Evergreen: {}", url);

    if !webview2_path.exists() {
        // Ensure the cache directory exists
        fs::create_dir_all(&cache_dir).expect("Failed to create cache directory");

        // Download the file
        let response = get(url).expect("Failed to download file");
        let mut file = fs::File::create(&webview2_path).expect("Failed to create file");
        let bytes = response.bytes().expect("Failed to read response bytes");
        file.write_all(&bytes).expect("Failed to write to file");
    }

    // Read the cached or newly downloaded file
    let webview2_data = fs::read(&webview2_path).expect("Failed to read file");
    println!(
        "  Loaded WebView2 Evergreen: {} ({} bytes)",
        WEBVIEW2_EVERGREEN_EXE,
        ByteSize(webview2_data.len().try_into().unwrap())
    );

    webview2_data
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_download_webview2_evergreen_download() {
        // Create a new mock server
        let mut server = Server::new();

        // Create a mock for the WebView2 download URL
        let mock = server
            .mock("GET", "/webview2")
            .with_status(200)
            .with_header("content-type", "application/octet-stream")
            .with_body(b"mock webview2 installer")
            .create();

        let mock_url = format!("{}/webview2", server.url());
        let cache_dir = tempdir().expect("Failed to create temp dir");
        let webview2_path = cache_dir.path().join(WEBVIEW2_EVERGREEN_EXE);

        // Ensure the cache directory is empty
        assert!(!webview2_path.exists());

        // Set the cache directory to the temporary directory
        std::env::set_var("CACHE_DIR", cache_dir.path());

        let result = std::panic::catch_unwind(|| {
            let bytes = download_webview2_evergreen_impl(&mock_url);
            assert_eq!(bytes, b"mock webview2 installer");
        });

        // Ensure the file is downloaded
        assert!(webview2_path.exists());
        assert!(result.is_ok());

        // Verify that the mock was called
        mock.assert();
    }

    #[test]
    fn test_download_webview2_evergreen_cached() {
        let cache_dir = tempdir().expect("Failed to create temp dir");
        let webview2_path = cache_dir.path().join(WEBVIEW2_EVERGREEN_EXE);

        // Write a mock installer to the cache
        {
            let mut file = fs::File::create(&webview2_path).expect("Failed to create file");
            file.write_all(b"mock cached installer")
                .expect("Failed to write to file");
        }

        // Ensure the cache directory is not empty
        assert!(webview2_path.exists());

        // Create a new mock server
        let mut server = Server::new();

        // Create a mock for the WebView2 download URL (it should not be called)
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/octet-stream")
            .with_body(b"mock webview2 installer")
            .expect(0)
            .create();

        let mock_url = server.url();
        std::env::set_var("MOCK_WEBVIEW2_URL", &mock_url);
        std::env::set_var("CACHE_DIR", cache_dir.path());

        let bytes = download_webview2_evergreen_impl(&mock_url);
        assert_eq!(bytes, b"mock cached installer");

        // Verify that the mock was not called
        mock.assert();
    }
}
