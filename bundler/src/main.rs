mod plugin_config;
mod webview2;

use twi_core::{BundleOptions, WebView2Embedding};
use bytesize::ByteSize;
use clap::Parser;
use colored::*;
use plugin_config::{load_tauri_config, Webview2Bundle};
use std::{env, path::Path, path::PathBuf};
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
    #[arg(short = 'c', long)]
    tauri_conf: String,

    /// Path to application executable or directory to bundle
    #[arg(short, long)]
    app: String,

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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("{}", "Packaging Tauri application...".green().bold());

    println!("  Loading config: {}", args.tauri_conf);
    let (tauri_conf, plugin_config) = load_tauri_config(&args.tauri_conf);
    let config_dir = Path::new(&args.tauri_conf).parent().unwrap();

    let title = args
        .title
        .clone()
        .or(tauri_conf.product_name.clone())
        .ok_or("--title required (or set productName in tauri.conf.json)")?;

    let icon = resolve_icon(&plugin_config, &tauri_conf, config_dir);
    let webview2 = resolve_webview2(&plugin_config);
    let sign_command = resolve_sign_command(&args, &plugin_config, &tauri_conf);

    let options = BundleOptions {
        setup_exe: load_embedded_setup(),
        name: tauri_conf.product_name.clone().unwrap_or_default(),
        title,
        version: tauri_conf
            .version
            .clone()
            .unwrap_or_else(|| "0.0.0".into()),
        identifier: tauri_conf.identifier.clone(),
        publisher: tauri_conf.bundle.publisher.clone().unwrap_or_default(),
        app: PathBuf::from(&args.app),
        main_exe: args.main_exe,
        icon,
        webview2,
        sign_command,
        output_dir: args
            .output_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| env::current_dir().unwrap()),
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
    tauri_conf: &tauri::Config,
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
) -> Option<WebView2Embedding> {
    match &plugin_config.webview2.bundle {
        Some(Webview2Bundle::Evergreen) => {
            println!(
                "  {}",
                "Bundling the webview2 evergreen bootstrapper...".green()
            );
            let data = download_webview2_evergreen();
            Some(WebView2Embedding {
                data,
                filename: WEBVIEW2_EVERGREEN_EXE.to_string(),
            })
        }
        None => {
            println!("  {}", "No webview2 bundle specified".blue());
            None
        }
    }
}

fn resolve_sign_command(
    args: &Args,
    plugin_config: &plugin_config::TauriWindowsInstaller,
    tauri_conf: &tauri::Config,
) -> Option<String> {
    // Priority: CLI flag > plugin config > tauri.conf.json bundle.windows.signCommand
    //
    // Tauri's sign_command uses %1 as a placeholder for the binary path.
    // Our library appends the path as the last argument instead.
    // If %1 appears in a non-final position, we can't safely rewrite it
    // (would change argument structure), so we strip it and let the library append.
    // If %1 doesn't appear, the command works as-is since the library appends the path.
    args.sign_command
        .clone()
        .or_else(|| plugin_config.sign_command.clone())
        .or_else(|| {
            tauri_conf
                .bundle
                .windows
                .sign_command
                .as_ref()
                .map(|sc| match sc {
                    tauri::utils::config::CustomSignCommandConfig::Command(cmd) => {
                        strip_percent1_placeholder(cmd)
                    }
                    tauri::utils::config::CustomSignCommandConfig::CommandWithOptions {
                        cmd,
                        args,
                    } => {
                        let mut parts = vec![cmd.clone()];
                        parts.extend(
                            args.iter()
                                .map(|a| strip_percent1_placeholder(a))
                                .filter(|a| !a.is_empty()),
                        );
                        parts.join(" ")
                    }
                })
        })
}

/// Strip the Tauri `%1` placeholder from a sign command string.
/// Returns the trimmed string with `%1` removed. If the entire arg is `%1`,
/// returns empty string (caller filters it out).
fn strip_percent1_placeholder(s: &str) -> String {
    s.replace("%1", "").trim().to_string()
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
