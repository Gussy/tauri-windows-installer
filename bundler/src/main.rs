mod plugin_config;
mod webview2;

use bundler::{
    manifest::SetupManifest, BUNDLE_RESOURCE, MANIFEST_RESOURCE, TWI_RESOURCE,
    WEBVIEW2_RESOURCE, WEBVIEW2_RESOURCE_FILENAME,
};
use bytesize::ByteSize;
use clap::Parser;
use colored::*;
use libsui::PortableExecutable;
use plugin_config::{load_tauri_config, Webview2Bundle};
use std::process::Command;
use std::{env, fs, path::Path};
use webview2::{download_webview2_evergreen, WEBVIEW2_EVERGREEN_EXE};

/// Tauri Windows Installer Bundler
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the Tauri configuration file
    #[arg(short = 'c', long)]
    tauri_conf: String,

    /// Path to application executable or directory to bundle
    #[arg(short, long)]
    app: String,

    /// Title of the bundled application
    #[arg(short, long)]
    title: String,

    /// Main executable name (required when --app is a directory, e.g. "my-app.exe")
    #[arg(long)]
    main_exe: Option<String>,

    /// Command to sign the output executable. The output file path is appended as the last argument.
    /// Example: --sign-command "signtool sign /fd SHA256 /f cert.pfx /p password"
    /// Can also be set via the "signCommand" field in the tauri-windows-installer plugin config.
    #[arg(short, long)]
    sign_command: Option<String>,
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

    // Create the tar bundle and determine the main executable name
    let app_path = Path::new(&args.app);
    let (bundle_data, main_exe_name) = if app_path.is_dir() {
        let main_exe = args.main_exe.as_deref().ok_or(
            "--main-exe is required when --app is a directory"
        )?;

        // Verify the main exe exists in the directory
        if !app_path.join(main_exe).exists() {
            return Err(format!(
                "Main executable '{}' not found in directory '{}'",
                main_exe,
                args.app
            ).into());
        }

        let bundle = create_tar_from_directory(app_path)?;
        println!(
            "  Bundled directory: {} ({}, main exe: {})",
            args.app,
            ByteSize(bundle.len() as u64),
            main_exe
        );
        (bundle, main_exe.to_string())
    } else {
        let exe_name = app_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let bundle = create_tar_from_file(app_path, &exe_name)?;
        println!(
            "  Bundled application: {} ({})",
            exe_name,
            ByteSize(bundle.len() as u64)
        );
        (bundle, exe_name)
    };

    // Create the manifest
    let manifest = SetupManifest {
        name: tauri_conf.product_name.clone().unwrap_or_default(),
        title: args.title,
        version: tauri_conf.version.clone().unwrap_or_else(|| "0.0.0".to_owned()),
        identifier: tauri_conf.identifier.clone(),
        application: main_exe_name,
        publisher: tauri_conf.bundle.publisher.clone().unwrap_or_default(),
    };

    // Write bundle data as a PE resource
    setup_pe = setup_pe.write_resource(BUNDLE_RESOURCE, bundle_data)?;

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
    drop(output_file);

    // Embed PE version info resources (FileVersion, ProductName, etc.)
    set_version_info(&output_filename, &manifest)?;

    // Sign the output executable if a sign command is provided
    // CLI flag takes precedence over plugin config
    let sign_command = args.sign_command.or(plugin_config.sign_command);
    if let Some(ref sign_cmd) = sign_command {
        sign_executable(sign_cmd, &output_filename)?;
    }

    // Print the output filename and size
    let output_size = fs::metadata(&output_filename)?.len();

    println!("{}", "Packaging complete.".green().bold());
    println!(
        "{}",
        format!("Created {} ({})", output_filename, ByteSize(output_size)).green()
    );

    Ok(())
}

fn create_tar_from_file(
    file_path: &Path,
    name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let data = fs::read(file_path)?;
    let mut builder = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();

    builder.append_data(&mut header, name, data.as_slice())?;
    Ok(builder.into_inner()?)
}

fn create_tar_from_directory(
    dir_path: &Path,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut builder = tar::Builder::new(Vec::new());
    builder.append_dir_all(".", dir_path)?;
    Ok(builder.into_inner()?)
}

fn set_version_info(
    output_path: &str,
    manifest: &SetupManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    use editpe::Image;
    use editpe::types::FixedFileInfo;

    let pe_data = fs::read(output_path)?;
    let mut image = Image::parse(pe_data)?;

    let mut resources = image.resource_directory().cloned().unwrap_or_default();

    // Parse version string "1.2.3" into VersionU32 (major = 1.2, minor = 3.0)
    let version = parse_version_u32(&manifest.version);

    let version_info = editpe::VersionInfo {
        info: FixedFileInfo {
            file_version: version,
            product_version: version,
            ..FixedFileInfo::default()
        },
        strings: vec![editpe::VersionStringTable {
            key: "040904B0".to_string(), // US English, Unicode
            strings: indexmap::indexmap! {
                "CompanyName".to_string() => manifest.publisher.clone(),
                "FileDescription".to_string() => format!("{} Setup", manifest.title),
                "FileVersion".to_string() => manifest.version.clone(),
                "InternalName".to_string() => format!("{}-setup", manifest.name),
                "OriginalFilename".to_string() => format!("{}-setup.exe", manifest.name),
                "ProductName".to_string() => manifest.title.clone(),
                "ProductVersion".to_string() => manifest.version.clone(),
            },
        }],
        vars: vec![],
    };

    resources.set_version_info(&version_info)?;
    image.set_resource_directory(resources)?;

    fs::write(output_path, image.data())?;
    println!("  Set version info: {}", manifest.version);

    Ok(())
}

fn parse_version_u32(version_str: &str) -> editpe::types::VersionU32 {
    let parts: Vec<u16> = version_str
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();

    let major = *parts.get(0).unwrap_or(&0);
    let minor = *parts.get(1).unwrap_or(&0);
    let patch = *parts.get(2).unwrap_or(&0);
    let build = *parts.get(3).unwrap_or(&0);

    // PE version format: major field = (major << 16) | minor, minor field = (patch << 16) | build
    editpe::types::VersionU32 {
        major: ((major as u32) << 16) | minor as u32,
        minor: ((patch as u32) << 16) | build as u32,
    }
}

fn sign_executable(
    sign_cmd: &str,
    file_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "  {}",
        format!("Signing executable with: {} {}", sign_cmd, file_path).green()
    );

    // Parse the sign command — first token is the program, rest are arguments
    let mut parts = shell_words::split(sign_cmd)
        .map_err(|e| format!("Failed to parse sign command: {}", e))?;

    if parts.is_empty() {
        return Err("Sign command is empty".into());
    }

    let program = parts.remove(0);
    parts.push(file_path.to_string());

    let output = Command::new(&program)
        .args(&parts)
        .output()
        .map_err(|e| format!("Failed to execute sign command '{}': {}", program, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "Sign command failed with exit code {}:\nstdout: {}\nstderr: {}",
            output.status.code().unwrap_or(-1),
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }

    println!("  {}", "Signing successful.".green());
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
