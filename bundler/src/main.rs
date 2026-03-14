mod plugin_config;
mod webview2;

use bundler::{
    manifest::SetupManifest, APPLICATION_RESOURCE, MANIFEST_RESOURCE, TWI_RESOURCE,
    WEBVIEW2_RESOURCE, WEBVIEW2_RESOURCE_FILENAME,
};
use bytesize::ByteSize;
use clap::Parser;
use colored::*;
use libsui::PortableExecutable;
use plugin_config::{load_tauri_config, Webview2Bundle};
use std::{env, fs, path::Path};
use webview2::{download_webview2_evergreen, WEBVIEW2_EVERGREEN_EXE};

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
    let (tauri_conf, plugin_config) = load_tauri_config(&args.tauri_conf);

    // Load the setup.exe file
    let setup_data = load_embedded_setup();

    // Create a PortableExecutable from the setup data
    let mut setup_pe = PortableExecutable::from(&setup_data)?;

    // Add an icon to the output executable
    let icon = plugin_config.icon.or_else(|| {
        tauri_conf
            .bundle
            .icon
            .iter()
            .find(|i| i.ends_with(".png"))
            .cloned()
    });
    if let Some(icon) = icon {
        let icon_path = Path::new(&args.tauri_conf).parent().unwrap().join(icon);
        let icon_data = fs::read(&icon_path)?;
        setup_pe = setup_pe.set_icon(&icon_data)?;
        println!(
            "  Added icon: {}",
            &icon_path.file_name().unwrap().to_str().unwrap()
        );
    } else {
        println!("  No icon specified, skipping icon addition");
    }

    // Add the application executable
    let app_exe = Path::new(&args.app)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap();
    let app_data = fs::read(&args.app)?;
    let app_size = app_data.len() as u64;
    println!(
        "  Loaded application executable: {} ({} bytes)",
        app_exe,
        ByteSize(app_size)
    );

    // Create the manifest
    let manifest = SetupManifest {
        name: tauri_conf.product_name.clone().unwrap_or_default(),
        title: args.title,
        version: tauri_conf.version.clone().unwrap_or_else(|| "0.0.0".to_owned()),
        identifier: tauri_conf.identifier.clone(),
        application: app_exe.to_owned(),
    };

    // Write application data as a PE resource
    setup_pe = setup_pe.write_resource(APPLICATION_RESOURCE, app_data)?;

    // Write manifest as a PE resource
    setup_pe = setup_pe.write_resource(MANIFEST_RESOURCE, manifest.to_binary()?)?;

    // Handle the webview2 bundling
    match &plugin_config.webview2.bundle {
        Some(Webview2Bundle::Evergreen) => {
            println!(
                "  {}",
                "Bundling the webview2 evergreen bootstrapper...".green()
            );

            let webview_data = download_webview2_evergreen();
            setup_pe = setup_pe.write_resource(WEBVIEW2_RESOURCE, webview_data)?;
            setup_pe = setup_pe.write_resource(
                WEBVIEW2_RESOURCE_FILENAME,
                WEBVIEW2_EVERGREEN_EXE.as_bytes().to_vec(),
            )?;
        }
        None => {
            println!("  {}", "No webview2 bundle specified".blue());
        }
    }

    // Write the TWI marker resource
    setup_pe = setup_pe.write_resource(TWI_RESOURCE, TWI_RESOURCE.as_bytes().to_vec())?;

    // Build the output executable
    let output_filename = format!("{}-setup.exe", manifest.name);
    let mut output_file = fs::File::create(&output_filename)?;
    setup_pe.build(&mut output_file)?;

    // Print the output filename and size
    let output_size = fs::metadata(&output_filename)?.len();

    println!("{}", "Packaging complete.".green().bold());
    println!(
        "{}",
        format!("Created {} ({})", output_filename, ByteSize(output_size)).green()
    );

    Ok(())
}

fn load_embedded_setup() -> Vec<u8> {
    let setup_data = include_bytes!(concat!(env!("OUT_DIR"), "/", env!("SETUP_EXE"))).to_vec();

    println!(
        "  Loaded setup executable: {} ({} bytes)",
        env!("SETUP_EXE"),
        ByteSize(setup_data.len().try_into().unwrap())
    );

    setup_data
}
