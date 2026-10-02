mod plugin_config;
mod webview2;

use bytesize::ByteSize;
use clap::Parser;
use colored::*;
use plugin_config::{load_tauri_config, Webview2Bundle};
use sha2::{Digest, Sha256};
use std::{env, path::Path, path::PathBuf};
use twi_core::bundle::SigningCommand;
use twi_core::{BundleOptions, WebView2Embedding};
use webview2::{download_webview2_evergreen, WEBVIEW2_EVERGREEN_EXE};

/// Tauri Windows Installer Bundler
///
/// Creates a self-extracting setup executable for a Tauri application.
/// Reads application metadata from tauri.conf.json and bundles the
/// application binary (or directory) into a setup.exe installer.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the Tauri configuration file
    #[arg(short = 'c', long, required_unless_present = "stub_info")]
    tauri_conf: Option<String>,

    /// Path to application executable or directory to bundle
    #[arg(short, long, required_unless_present = "stub_info")]
    app: Option<String>,

    /// Title of the bundled application. Falls back to productName from tauri.conf.json.
    #[arg(short, long)]
    title: Option<String>,

    /// Main executable name (required when --app is a directory, e.g. "my-app.exe")
    #[arg(long)]
    main_exe: Option<String>,

    /// Command to sign the output executable. The output file path is appended as the last argument.
    /// Example: --sign-command "signtool sign /fd SHA256 /f cert.pfx /p password"
    /// Can also be set via the "signCommand" field in the tauri-windows-installer plugin config,
    /// or via bundle.windows.signCommand in tauri.conf.json.
    #[arg(short, long)]
    sign_command: Option<String>,

    /// Output directory for the setup executable (defaults to current directory)
    #[arg(short, long)]
    output_dir: Option<String>,

    /// Explicit Windows setup stub, usable on any build host.
    #[arg(long)]
    setup_exe: Option<PathBuf>,

    /// Inspect the embedded or explicitly supplied setup stub as JSON.
    #[arg(long)]
    stub_info: bool,

    /// WebView2 bootstrapper cache directory.
    #[arg(long)]
    cache_dir: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let setup = load_setup(args.setup_exe.as_deref())?;
    if args.stub_info {
        let image = editpe::Image::parse(setup.as_slice())?;
        let manifest = image
            .resource_directory()
            .and_then(|r| r.get_manifest().ok().flatten());
        println!(
            "{}",
            serde_json::json!({
                "stub_sha256":format!("{:x}",Sha256::digest(&setup)),
                "package_version":env!("CARGO_PKG_VERSION"),
                "format_version":twi_core::FORMAT_VERSION,
                "architecture":"x86_64",
                "manifest_present":manifest.is_some(),
                "manifest_as_invoker":manifest.as_ref().is_some_and(|m|m.contains("asInvoker")),
                "abi_compatible":true,
            })
        );
        return Ok(());
    }
    println!("{}", "Packaging Tauri application...".green().bold());

    let config_path = args.tauri_conf.as_deref().ok_or("--tauri-conf required")?;
    println!("  Loading config: {config_path}");
    let (tauri_conf, plugin_config) = load_tauri_config(config_path)?;
    let config_dir = Path::new(config_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));

    let title = args
        .title
        .clone()
        .or(tauri_conf.product_name.clone())
        .ok_or("--title required (or set productName in tauri.conf.json)")?;

    let icon = resolve_icon(&plugin_config, &tauri_conf, config_dir);
    let webview2 = resolve_webview2(&plugin_config, args.cache_dir.as_deref())?;
    let sign_command = resolve_sign_command(&args, &plugin_config, &tauri_conf)?;

    let desktop_shortcut = plugin_config.desktop_shortcut.unwrap_or(true);

    let options = BundleOptions {
        setup_exe: setup,
        name: tauri_conf
            .product_name
            .clone()
            .unwrap_or_else(|| title.clone()),
        title,
        version: tauri_conf.version.clone().unwrap_or_else(|| "0.0.0".into()),
        identifier: tauri_conf.identifier.clone(),
        publisher: tauri_conf.bundle.publisher.clone().unwrap_or_default(),
        app: PathBuf::from(args.app.as_deref().ok_or("--app required")?),
        main_exe: args.main_exe,
        icon,
        webview2,
        sign_command,
        desktop_shortcut,
        output_dir: args
            .output_dir
            .map(PathBuf::from)
            .unwrap_or(env::current_dir()?),
        on_progress: Some(Box::new(|msg| println!("  {}", msg.green()))),
    };

    let output = twi_core::bundle(options)?;

    println!("{}", "Packaging complete.".green().bold());
    println!(
        "{}",
        format!(
            "Created {} ({})",
            output.path.display(),
            ByteSize(output.size)
        )
        .green()
    );

    Ok(())
}

fn resolve_icon(
    plugin_config: &plugin_config::TauriWindowsInstaller,
    tauri_conf: &tauri_utils::config::Config,
    config_dir: &Path,
) -> Option<PathBuf> {
    let icon_relative = plugin_config.icon.clone().or_else(|| {
        tauri_conf
            .bundle
            .icon
            .iter()
            .find(|i| i.ends_with(".png"))
            .cloned()
    });

    icon_relative.map(|icon| config_dir.join(icon))
}

fn resolve_webview2(
    plugin_config: &plugin_config::TauriWindowsInstaller,
    cache: Option<&Path>,
) -> Result<Option<WebView2Embedding>, Box<dyn std::error::Error>> {
    match &plugin_config.webview2.bundle {
        Some(Webview2Bundle::Evergreen) => {
            println!(
                "  {}",
                "Bundling the webview2 evergreen bootstrapper...".green()
            );
            let data = download_webview2_evergreen(cache)?;
            Ok(Some(WebView2Embedding {
                data,
                filename: WEBVIEW2_EVERGREEN_EXE.to_string(),
            }))
        }
        None => {
            println!("  {}", "No webview2 bundle specified".blue());
            Ok(None)
        }
    }
}

fn resolve_sign_command(
    args: &Args,
    plugin_config: &plugin_config::TauriWindowsInstaller,
    tauri_conf: &tauri_utils::config::Config,
) -> Result<Option<SigningCommand>, twi_core::BundleError> {
    if let Some(command) = args
        .sign_command
        .as_ref()
        .or(plugin_config.sign_command.as_ref())
    {
        return SigningCommand::parse_legacy(command).map(Some);
    }
    use tauri_utils::config::CustomSignCommandConfig;
    tauri_conf
        .bundle
        .windows
        .sign_command
        .as_ref()
        .map(|command| match command {
            CustomSignCommandConfig::Command(command) => SigningCommand::parse_legacy(command),
            CustomSignCommandConfig::CommandWithOptions { cmd, args } => Ok(SigningCommand {
                program: PathBuf::from(cmd),
                args: args.clone(),
            }),
        })
        .transpose()
}

fn load_setup(path: Option<&Path>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let setup = if let Some(path) = path {
        std::fs::read(path)?
    } else {
        let bytes = include_bytes!(concat!(env!("OUT_DIR"), "/", env!("SETUP_EXE"))).to_vec();
        if bytes.is_empty() {
            return Err("No setup stub embedded. Build twi_installer and set TWI_SETUP_EXE, or pass --setup-exe.".into());
        }
        if format!("{:x}", Sha256::digest(&bytes)) != env!("TWI_SETUP_EXE_SHA256") {
            return Err("Embedded setup stub checksum does not match build metadata".into());
        }
        bytes
    };
    twi_core::bundle::validate_setup_stub(&setup)?;
    Ok(setup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Args {
        Args {
            tauri_conf: None,
            app: None,
            title: None,
            main_exe: None,
            sign_command: None,
            output_dir: None,
            setup_exe: None,
            stub_info: false,
            cache_dir: None,
        }
    }

    #[test]
    fn structured_signer_preserves_program_arguments_and_placeholder() {
        let config: tauri_utils::config::Config = serde_json::from_value(serde_json::json!({
            "identifier":"com.example.test",
            "bundle":{"windows":{"signCommand":{"cmd":"C:\\Program Files\\sign.exe",
            "args":["--password","two words","--input=%1","--quiet"]}}}
        }))
        .unwrap();
        let command = resolve_sign_command(
            &args(),
            &plugin_config::TauriWindowsInstaller::default(),
            &config,
        )
        .unwrap()
        .unwrap();
        assert_eq!(command.program, PathBuf::from(r"C:\Program Files\sign.exe"));
        assert_eq!(
            command.args,
            ["--password", "two words", "--input=%1", "--quiet"]
        );
    }

    #[test]
    fn cli_signer_takes_priority() {
        let config: tauri_utils::config::Config =
            serde_json::from_value(serde_json::json!({"identifier":"com.example.test"})).unwrap();
        let mut input = args();
        input.sign_command = Some("cli.exe %1".into());
        let plugin = plugin_config::TauriWindowsInstaller {
            sign_command: Some("plugin.exe".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_sign_command(&input, &plugin, &config)
                .unwrap()
                .unwrap()
                .program,
            PathBuf::from("cli.exe")
        );
    }
}
