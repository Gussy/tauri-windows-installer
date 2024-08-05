use reqwest::blocking::get;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const WEBVIEW2_EVERGREEN_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
const WEBVIEW2_EVERGREEN_EXE: &str = "MicrosoftEdgeWebview2Setup.exe";

fn main() {
    let config_path = "config.toml";
    let config_contents = fs::read_to_string(config_path).expect("Failed to read config file");
    let config: toml::Value =
        toml::from_str(&config_contents).expect("Failed to parse config file");

    // Get the external program filename and path
    let external_program = config["external"]["program"]
        .as_str()
        .expect("Failed to get external program filename");
    let external_path = config["external"]["path"]
        .as_str()
        .expect("Failed to get external program path");

    // Set environment variables for the program
    let app_id = config["external"]["app_id"]
        .as_str()
        .expect("Failed to get application id");
    println!("cargo:rustc-env=BUNDLED_APP_NAME={}", external_program);
    println!("cargo:rustc-env=BUNDLED_APP_ID={}", app_id);

    // Copy the external program to the output directory
    let source_path = PathBuf::from(external_path).join(external_program);
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = PathBuf::from(&out_dir).join(external_program);
    fs::copy(&source_path, &dest_path).expect("Failed to copy external program");

    println!("cargo:rerun-if-changed={}", source_path.display());
    println!("cargo:rerun-if-changed={}", config_path);

    // Check if WebView2 runtime should be bundled
    let bundle_webview2 = config["installer"]["bundle_webview2"]
        .as_bool()
        .expect("Failed to get bundle_webview2 value");

    if bundle_webview2 {
        // Download and bundle WebView2 runtime
        let out_dir = env::var("OUT_DIR").unwrap();
        let webview2_path = PathBuf::from(&out_dir).join(WEBVIEW2_EVERGREEN_EXE);

        if !webview2_path.exists() {
            let response = get(WEBVIEW2_EVERGREEN_URL).expect("Failed to download file");
            let mut file = fs::File::create(webview2_path).expect("Failed to create file");
            let bytes = response.bytes().expect("Failed to read response bytes");
            file.write_all(&bytes).expect("Failed to write to file");
        }

        println!("cargo:rustc-env=WEBVIEW2_BUNDLED=true");
        println!(
            "cargo:rustc-env=WEBVIEW2_BUNDLED_NAME={}",
            WEBVIEW2_EVERGREEN_EXE
        );
    } else {
        println!("cargo:rustc-env=WEBVIEW2_BUNDLED=false");
        println!("cargo:rustc-env=WEBVIEW2_BUNDLED_NAME=");
    }
}
