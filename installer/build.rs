use embed_manifest::{embed_manifest, new_manifest};
use reqwest::blocking::get;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const WEBVIEW2_EVERGREEN_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
const WEBVIEW2_EVERGREEN_EXE: &str = "MicrosoftEdgeWebview2Setup.exe";

fn main() {
    // Embed the windows app.manifest
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("app.manifest")).expect("unable to embed manifest file");
        println!("cargo:rerun-if-changed=app.manifest");
    }

    let config_path = "config.toml";
    let config_contents = fs::read_to_string(config_path).expect("Failed to read config file");
    let config: toml::Value =
        toml::from_str(&config_contents).expect("Failed to parse config file");

    // Get the application filename and path
    let application_exe = config["application"]["program"]
        .as_str()
        .expect("Failed to get application filename");
    let application_path = config["application"]["path"]
        .as_str()
        .expect("Failed to get application path");

    // Set environment variables for the program
    let app_id = config["application"]["id"]
        .as_str()
        .expect("Failed to get application id");
    let app_name = config["application"]["name"]
        .as_str()
        .expect("Failed to get application name");
    let app_version = config["application"]["version"]
        .as_str()
        .expect("Failed to get application version");
    println!("cargo:rustc-env=BUNDLED_APP_EXE={}", application_exe);
    println!("cargo:rustc-env=BUNDLED_APP_NAME={}", app_name);
    println!("cargo:rustc-env=BUNDLED_APP_VERSION={}", app_version);
    println!("cargo:rustc-env=BUNDLED_APP_ID={}", app_id);

    // Copy the application to the output directory
    let source_path = PathBuf::from(application_path).join(application_exe);
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = PathBuf::from(&out_dir).join(application_exe);
    fs::copy(&source_path, &dest_path).expect("Failed to copy application exe");

    println!("cargo:rerun-if-changed={}", source_path.display());
    println!("cargo:rerun-if-changed={}", config_path);

    // Check if WebView2 runtime should be bundled
    let bundle_webview2 = config["webview2"]["bundle"]
        .as_str()
        .expect("Failed to get bundle_webview2 value");

    if bundle_webview2 == "evergreen" {
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
