use dotenvy::dotenv;
use embed_manifest::{embed_manifest, new_manifest};
use reqwest::blocking::get;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const WEBVIEW2_EVERGREEN_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
const WEBVIEW2_EVERGREEN_EXE: &str = "MicrosoftEdgeWebview2Setup.exe";

fn main() {
    // Load environment variables from the .env file
    dotenv().ok();

    // Embed the app.manifest file
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("app.manifest")).expect("unable to embed manifest file");
        println!("cargo:rerun-if-changed=app.manifest");
    }

    // Get the OUT_DIR environment variable
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable not set");

    // Determine if the build is debug or release
    let profile = env::var("PROFILE").expect("PROFILE environment variable not set");

    // Determine the build directory (target/debug or target/release)
    let target_dir = PathBuf::from("target").join(&profile);

    // Get the tauri config path from the environment variables
    let tauri_conf_path = PathBuf::from(get_env_var("TAURI_CONF_PATH", None))
        .canonicalize()
        .expect("Failed to get tauri config path");
    let tauri_path_binding = PathBuf::from(tauri_conf_path.clone());
    let tauri_path = tauri_path_binding
        .parent()
        .expect("Failed to get tauri root path");
    println!("cargo:rerun-if-changed={}", tauri_conf_path.display());

    // Get the build target path from the environment variables
    let build_target_base: &str = &get_env_var("BUILD_TARGET_PATH", Some(".."));
    let build_target_path = PathBuf::from(build_target_base).join(&target_dir);

    // Read and parse the tauri.conf.json file
    let tauri_conf_contents =
        fs::read_to_string(tauri_conf_path).expect("Failed to read tauri config file");
    let tauri_conf: tauri::Config =
        serde_json::from_str(&tauri_conf_contents).expect("Failed to parse tauri config file");

    // Get the tauri config values
    let product_name: &str = &tauri_conf
        .product_name
        .expect("Failed to get tauri config product_name");
    let application_exe: &str = &format!("{}{}", product_name, env::consts::EXE_SUFFIX);
    let version = tauri_conf
        .version
        .expect("Failed to get tauri config version");

    // Set the environment variables for the bundled application
    println!("cargo:rustc-env=BUNDLED_APP_NAME={}", product_name);
    println!("cargo:rustc-env=BUNDLED_APP_EXE={}", application_exe);
    println!("cargo:rustc-env=BUNDLED_APP_VERSION={}", version);
    println!("cargo:rustc-env=BUNDLED_APP_ID={}", tauri_conf.identifier);

    // Copy the application to the output directory
    let source_path = PathBuf::from(build_target_path).join(application_exe);
    let dest_path = PathBuf::from(&out_dir).join(application_exe);
    fs::copy(&source_path, &dest_path).expect("Failed to copy application exe");
    println!("cargo:rerun-if-changed={}", source_path.display());
    println!("cargo:rerun-if-changed={}", application_exe);

    // Check if WebView2 runtime should be bundled
    let bundle_webview2 = tauri_conf
        .plugins
        .0
        .get(env!("CARGO_PKG_NAME"))
        .and_then(|p| p.get("webview2"))
        .and_then(|w| w.get("bundle"))
        .expect("Failed to get webview2 bundle value");

    // Bundle WebView2 runtime if the value is "evergreen"
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

    // Get the icon path from the tauri configuration
    let icon_path_str = tauri_conf
        .plugins
        .0
        .get(env!("CARGO_PKG_NAME"))
        .and_then(|p| p.get("icon"))
        .expect("Failed to get webview2 bundle value")
        .as_str()
        .unwrap();
    let icon_path = PathBuf::from(&icon_path_str);
    let full_icon_path = PathBuf::from(&tauri_path).join(&icon_path);
    let formatted_path = full_icon_path.to_str().unwrap().replace(r"\", r"\\");

    // Generate the app.rc file
    let rc_contents = format!(
        r#"
        32512 ICON "{}"
    "#,
        formatted_path
    );
    // TODO: Above is a hard-coded hack and would be cleaner and more complete if integrated with tauri

    let rc_path = PathBuf::from(&out_dir).join("resource.rc");
    fs::write(&rc_path, rc_contents).expect("Failed to write resource.rc");

    // Compile the resource file
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_resource::compile(rc_path.to_str().unwrap(), embed_resource::NONE);
        println!("cargo:rerun-if-changed={}", icon_path.display());
    }
}

/// Gets an environment variable from the environment or .env file, or returns a default value if not found.
/// If no default value is provided and the environment variable is not found, an panic will occur
fn get_env_var(key: &str, default: Option<&str>) -> String {
    match env::var(key) {
        Ok(val) => val,
        Err(_) => {
            if let Some(default) = default {
                default.to_string()
            } else {
                panic!(
                    "Environment variable {} not found and no default value provided",
                    key
                );
            }
        }
    }
}
