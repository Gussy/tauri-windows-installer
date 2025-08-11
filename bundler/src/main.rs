pub mod plugin_config;
pub mod webview2;

use crate::plugin_config::{load_tauri_config, Webview2Bundle};
use crate::webview2::{download_webview2_evergreen, WEBVIEW2_EVERGREEN_EXE};

use bundler::{
    manifest::SetupManifest, APPLICATION_RESOURCE, MANIFEST_RESOURCE, TWI_RESOURCE,
    WEBVIEW_RESOURCE, WEBVIEW_RESOURCE_FILENAME,
};
use bytesize::ByteSize;
use clap::Parser;
use colored::*;
use libsui::PortableExecutable;
use std::{env, fs::File, path::Path};

/// Tauri Windows Installer Bundler
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the Tauri configuration file
    #[arg(short = 'c', long)]
    tauri_conf: String,

    /// Path to application to bundle
    #[arg(short, long)]
    app: String,

    /// Title of the bundled application
    #[arg(short, long)]
    title: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("{}", "Packaging Tauri application...".green().bold());

    println!("  Loading config: {}", args.tauri_conf);
    let (tauri_conf, plugin_config) =
        load_tauri_config(&args.tauri_conf).expect("Failed to load tauri config");

    // Load the setup.exe file
    let setup_data = load_embedded_setup()?;

    // Create the portable executable
    let mut setup_pe = PortableExecutable::from(&setup_data)?;

    // Handle the webview2 bundling
    if let Some(Webview2Bundle::Evergreen) = &plugin_config.webview2.bundle {
        println!(
            "  {}",
            "Bundling the webview2 evergreen bootstrapper...".green()
        );

        let webview_data = download_webview2_evergreen()?;
        setup_pe = setup_pe
            .write_resource(WEBVIEW_RESOURCE, webview_data.to_vec())?
            .write_resource(
                WEBVIEW_RESOURCE_FILENAME,
                WEBVIEW2_EVERGREEN_EXE.as_bytes().to_vec(),
            )?;
    } else {
        println!("  {}", "No webview2 bundle specified".blue());
    }

    // Add the application executable to the package
    let app_exe = Path::new(&args.app)
        .file_name()
        .ok_or("Invalid application path")?
        .to_str()
        .ok_or("Failed to convert app filename to str")?;
    let app_data = std::fs::read(&args.app)?;
    let app_size: u64 = app_data.len() as u64;
    setup_pe = setup_pe.write_resource(APPLICATION_RESOURCE, app_data)?;
    println!(
        "  Loaded application executable: {} ({} bytes)",
        app_exe,
        ByteSize(app_size)
    );

    // Create and add a manifest
    let manifest = SetupManifest {
        name: tauri_conf
            .product_name
            .clone()
            .unwrap_or_else(|| "".to_string()),
        title: args.title,
        version: tauri_conf
            .version
            .clone()
            .unwrap_or_else(|| "0.0.0".to_string()),
        identifier: tauri_conf.identifier.clone(),
        application: app_exe.to_owned(),
    };
    let manifest_data = manifest.to_binary()?;
    setup_pe = setup_pe.write_resource(MANIFEST_RESOURCE, manifest_data)?;

    // Add the TWI resource
    setup_pe = setup_pe.write_resource(TWI_RESOURCE, TWI_RESOURCE.as_bytes().to_vec())?;

    // // Use the icon specified in the plugin config, or the first png icon in the bundle config
    // if let Some(icon) = plugin_config.icon.or_else(|| {
    //     tauri_conf
    //         .bundle
    //         .icon
    //         .iter()
    //         .find(|i| i.ends_with(".png"))
    //         .cloned()
    // }) {
    //     let icon_path = Path::new(&args.tauri_conf).parent().unwrap().join(&icon);
    //     let icon_data = std::fs::read(&icon_path)?;
    //     setup_pe = setup_pe.set_icon(&icon_data).expect("Failed to set icon");
    //     println!(
    //         "  Added icon: {}",
    //         icon_path.file_name().unwrap().to_str().unwrap()
    //     );
    // } else {
    //     println!("  No icon specified, skipping icon addition");
    // }

    // // Add the application manifest
    // let manifest_data = include_bytes!(concat!(env!("OUT_DIR"), "\\", env!("MANIFEST_FILENAME")));
    // let manifest_str =
    //     std::str::from_utf8(manifest_data).expect("Manifest data is not valid UTF-8");
    // setup_pe = setup_pe
    //     .write_manifest(manifest_str)
    //     .expect("Failed to write manifest");

    // Package the executable with the added files and manifest
    let output_filename = format!("{}-setup.exe", manifest.name);
    let mut output_file = File::create(&output_filename)?;
    setup_pe.build(&mut output_file)?;
    drop(output_file);

    // Print the output filename and size
    let output_size = std::fs::metadata(&output_filename)?.len();
    println!("{}", "Packaging complete.".green().bold());
    println!(
        "{}",
        format!("Created {} ({})", output_filename, ByteSize(output_size)).green()
    );

    Ok(())
}

fn load_embedded_setup() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let setup_data = include_bytes!(concat!(env!("OUT_DIR"), "\\", env!("SETUP_EXE"))).to_vec();

    println!(
        "  Loaded setup executable: {} ({} bytes)",
        env!("SETUP_EXE"),
        ByteSize(setup_data.len() as u64)
    );

    Ok(setup_data)
}
