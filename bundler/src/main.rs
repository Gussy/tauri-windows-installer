mod exe_packager;
mod metadata;
mod plugin_config;
mod webview2;

use crate::webview2::download_webview2_evergreen;

use bytesize::ByteSize;
use clap::Parser;
use colored::*;
use exe_packager::{ExePackager, SetupManifest};
use plugin_config::{load_tauri_config, Webview2Bundle};
use std::{env, path::Path};
use webview2::WEBVIEW2_EVERGREEN_EXE;

/// Tauri Windows Installer Bundler
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the Tauri configuration file
    #[arg(short, long)]
    tauri_conf: String,

    /// Path to application to bundle
    #[arg(short, long)]
    app: String,
}

fn main() {
    let args = Args::parse();

    println!("{}", "Packaging Tauri application...".green().bold());

    println!("  Loading config: {}", args.tauri_conf);
    let (tauri_conf, plugin_config) = load_tauri_config(&args.tauri_conf);

    // Load the setup.exe file
    let setup_data = load_embedded_setup();

    // Create the packager
    let mut packager = ExePackager::new(setup_data);

    // Handle the webview2 bundling
    // let webview_data: Vec<u8>;
    match &plugin_config.webview2.bundle {
        Some(Webview2Bundle::Evergreen) => {
            println!(
                "  {}",
                "Bundling the webview2 evergreen bootstrapper...".green()
            );

            let webview_data = download_webview2_evergreen();
            packager.add_file(WEBVIEW2_EVERGREEN_EXE, webview_data.to_vec());
        }
        None => {
            println!("  {}", "No webview2 bundle specified".blue());
        }
    }

    // Add the application executable to the package
    let app_exe = Path::new(&args.app).file_name().unwrap().to_str().unwrap();
    let app_data = std::fs::read(&args.app).expect("Failed to read application executable");
    let app_size: u64 = app_data
        .len()
        .try_into()
        .expect("Failed to convert app data length");
    packager.add_file(app_exe, app_data);
    println!(
        "  Loaded application executable: {} ({} bytes)",
        app_exe,
        ByteSize(app_size)
    );

    // Create and add a manifest
    let manifest = SetupManifest {
        name: tauri_conf.product_name.clone().unwrap_or("".to_owned()),
        version: tauri_conf.version.clone().unwrap_or("0.0.0".to_owned()),
        identifier: tauri_conf.identifier.clone(),
        application: app_exe.to_owned(),
    };
    packager.add_manifest(&manifest);

    // Package the executable with the added files and manifest
    let output_filename = format!("{}-setup.exe", manifest.name);
    packager.package(Path::new(&output_filename));

    let output_size = std::fs::metadata(&output_filename)
        .expect("Failed to get output file metadata")
        .len();

    println!("{}", "Packaging complete.".green().bold());
    println!(
        "{}",
        format!("Created {} ({})", output_filename, ByteSize(output_size)).green()
    );
}

fn load_embedded_setup() -> Vec<u8> {
    let setup_data = include_bytes!(concat!(env!("OUT_DIR"), "\\", env!("SETUP_EXE"))).to_vec();

    println!(
        "  Loaded setup executable: {} ({} bytes)",
        env!("SETUP_EXE"),
        ByteSize(setup_data.len().try_into().unwrap())
    );

    setup_data
}

// fn load_executable_from_output_dir(filename: &str) -> Vec<u8> {
//     let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable not set");
//     let executable_path = PathBuf::from(&out_dir).join(filename);
//     let mut executable_data = Vec::new();
//     let mut executable_file =
//         File::open(&executable_path).expect("Failed to open setup executable");
//     executable_file
//         .read_to_end(&mut executable_data)
//         .expect("Failed to read setup executable");
//     println!(
//         "  Loaded setup executable: {} ({} bytes)",
//         executable_path
//             .file_name()
//             .expect("Failed to get filename")
//             .to_str()
//             .expect("Failed to convert filename to string"),
//         ByteSize(executable_data.len().try_into().unwrap())
//     );

//     executable_data
// }
