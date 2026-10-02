#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(any(windows, test))]
mod archive;
#[cfg(windows)]
mod bundle;
#[cfg(any(windows, test))]
mod executable;
#[cfg(windows)]
mod process;
#[cfg(any(windows, test))]
mod transaction;
#[cfg(windows)]
mod windows;

// The bundler rejects stubs that cannot understand the current manifest envelope.
#[used]
static SETUP_ABI_MARKER: [u8; 17] = *b"TWI_SETUP_ABI_V1\0";

#[cfg(not(windows))]
fn main() {
    eprintln!("This installer is Windows-only.");
    std::process::exit(1);
}

#[cfg(windows)]
#[derive(Default)]
struct Options {
    no_launch: bool,
}

#[cfg(windows)]
fn main() {
    use std::sync::Arc;
    std::hint::black_box(&SETUP_ABI_MARKER);
    let silent = std::env::args_os().any(|argument| argument == "--silent");
    let logger = Arc::new(Logger::new());
    let panic_logger = logger.clone();
    std::panic::set_hook(Box::new(move |panic| {
        panic_logger.record(&format!("PANIC: {panic}"))
    }));
    logger.record("Installer started.");
    let outcome = std::panic::catch_unwind(|| -> Result<(), String> {
        let mut options = Options::default();
        for argument in std::env::args_os().skip(1) {
            match argument.to_str() {
                Some("--silent") => {}
                Some("--no-launch") => options.no_launch = true,
                _ => {
                    return Err(format!(
                        "Unsupported installer argument: {}",
                        argument.to_string_lossy()
                    ))
                }
            }
        }
        run_installer(&logger, &options)
    })
    .unwrap_or_else(|_| {
        Err(
            "The installer stopped unexpectedly. Rerun it to recover any pending transaction."
                .into(),
        )
    });
    if let Err(error) = outcome {
        logger.record(&format!("FAILED: {error}"));
        let message = format!("{error}\n\n{}", logger.location());
        eprintln!("{message}");
        if !silent {
            windows::show_error(&message);
        }
        std::process::exit(1);
    }
    logger.record("Installation completed successfully.");
}

#[cfg(windows)]
fn run_installer(logger: &Logger, options: &Options) -> Result<(), String> {
    use std::fs;
    windows::check_os_version().map_err(display_error)?;
    if !twi_core::is_bundled() {
        return Err("This executable is not a valid TWI setup package.".into());
    }
    let manifest = twi_core::get_manifest()?;
    twi_core::validate_manifest(&manifest)?;
    logger.record(&format!(
        "Package {} {} ({})",
        manifest.title, manifest.version, manifest.identifier
    ));
    let appdata = windows::get_local_app_data().map_err(display_error)?;
    let programs = appdata.join("Programs");
    twi_core::reject_reparse_point(&programs)?;
    fs::create_dir_all(&programs)
        .map_err(|error| format!("Cannot create Programs directory: {error}"))?;
    let root = twi_core::validate_install_root(&programs, &manifest.identifier)?;
    let _lock = twi_core::acquire_install_lock(&root)?;
    let pending_uninstall = programs.join(format!("{}.twi-uninstall.json", manifest.identifier));
    twi_core::reject_reparse_point(&pending_uninstall)?;
    if pending_uninstall
        .try_exists()
        .map_err(|error| error.to_string())?
    {
        return Err("An uninstall is pending. Retry uninstall from Apps & Features before installing this application again.".into());
    }
    // Windows holds a directory handle for the current working directory. Keep
    // this process outside the tree it will replace, including when setup lives there.
    std::env::set_current_dir(&programs)
        .map_err(|error| format!("Cannot set installer working directory: {error}"))?;
    let mut restore = |snapshot: &serde_json::Value| {
        windows::restore_registration(snapshot, &manifest.identifier)
    };
    logger.record("Recovering any previous interrupted installation.");
    transaction::recover_with_stop(&root, &mut restore, &mut |path| {
        process::find_and_kill_processes_from_directory(path).map_err(display_error)
    })?;
    twi_core::validate_install_root(&programs, &manifest.identifier)?;
    let previous = previous_installation(&root, &manifest.identifier)?;
    let bundle_data = twi_core::get_bundle_data()?;
    archive::verify_digest(bundle_data, &manifest.bundle_sha256)?;
    logger.record("Validating application archive and available storage.");
    let unit = windows::allocation_unit(&programs).map_err(display_error)?;
    let info = archive::inspect(bundle_data, manifest.unpacked_size, unit)?;
    let required = info
        .allocation_size
        .checked_add(1024 * 1024)
        .ok_or_else(|| "Installation storage size overflow.".to_string())?;
    let available = windows::get_free_space(&programs).map_err(display_error)?;
    if available < required {
        return Err(format!(
            "{} requires {} bytes of available storage; only {} bytes are available to this user.",
            manifest.title, required, available
        ));
    }
    let previous_owned_title = match &previous {
        Some(metadata) => match metadata.owned_shortcut_title() {
            Some(title)
                if windows::shortcut_owned(title, &root, &metadata.app_exe)
                    .map_err(display_error)? =>
            {
                Some(title.to_string())
            }
            _ => None,
        },
        None => None,
    };
    // Preserve the user's deliberate deletion/replacement of a previously owned
    // shortcut. Enabling shortcuts after a previously disabled install creates one.
    let create_shortcut = manifest.desktop_shortcut
        && match &previous {
            Some(metadata) if metadata.desktop_shortcut => previous_owned_title.is_some(),
            _ => true,
        };
    if create_shortcut
        && windows::desktop_shortcut_exists(&manifest.title).map_err(display_error)?
    {
        let owned = previous_owned_title
            .as_ref()
            .is_some_and(|title| title.eq_ignore_ascii_case(&manifest.title));
        if !owned {
            return Err(format!("A desktop shortcut named {} already belongs to another application or was customized. It will not be overwritten.", manifest.title));
        }
    }
    let mut titles = Vec::new();
    if let Some(title) = &previous_owned_title {
        titles.push(title.clone());
    }
    if create_shortcut {
        titles.push(manifest.title.clone());
    }
    let snapshot = windows::snapshot_registration(&manifest.identifier, &root, &titles)
        .map_err(display_error)?;
    let recovery_space = (serde_json::to_vec(&snapshot)
        .map_err(|error| error.to_string())?
        .len() as u64)
        .checked_mul(2)
        .and_then(|size| size.checked_add(required))
        .ok_or_else(|| "Installation recovery storage size overflow.".to_string())?;
    let available = windows::get_free_space(&programs).map_err(display_error)?;
    if available < recovery_space {
        return Err(format!("Insufficient storage for the application and durable recovery journal: {recovery_space} bytes required, {available} available."));
    }
    let mut transaction = transaction::Transaction::begin(&root, snapshot)?;
    let install = (|| -> Result<Option<String>, String> {
        let stage = transaction.stage()?;
        logger.record(&format!(
            "Extracting validated application to {}",
            stage.display()
        ));
        archive::extract(bundle_data, manifest.unpacked_size, &stage)?;
        let executable = twi_core::executable_path(&stage, &manifest.application)?;
        executable::validate(&executable)?;
        let metadata = twi_core::InstallMetadata {
            format_version: twi_core::FORMAT_VERSION,
            app_title: manifest.title.clone(),
            app_id: manifest.identifier.clone(),
            app_exe: manifest.application.clone(),
            version: manifest.version.clone(),
            desktop_shortcut: create_shortcut,
            shortcut_title: create_shortcut.then(|| manifest.title.clone()),
        };
        metadata.validate()?;
        transaction::atomic_write(
            &stage.join(twi_core::INSTALL_METADATA_FILENAME),
            &serde_json::to_vec_pretty(&metadata).map_err(|error| error.to_string())?,
        )?;
        logger.record("Checking WebView2 prerequisite.");
        bundle::WebView2::load()
            .map_err(display_error)?
            .ensure_installed()
            .map_err(display_error)?;
        twi_core::validate_install_root(&programs, &manifest.identifier)?;
        if root.try_exists().map_err(|error| error.to_string())? {
            logger.record("Stopping existing application processes.");
            process::find_and_kill_processes_from_directory(&root).map_err(display_error)?;
        }
        logger.record("Switching to the prepared installation.");
        transaction.switch()?;
        twi_core::validate_install_root(&programs, &manifest.identifier)?;
        windows::write_uninstall_entry(&manifest, &root, info.unpacked_size)
            .map_err(display_error)?;
        if create_shortcut {
            windows::create_desktop_shortcut(&manifest, &root).map_err(display_error)?;
        }
        if let Some(title) = &previous_owned_title {
            if !create_shortcut || !title.eq_ignore_ascii_case(&manifest.title) {
                windows::remove_desktop_shortcut(title).map_err(display_error)?;
            }
        }
        if !options.no_launch {
            logger.record("Launching the installed application.");
            process::spawn_detached_process(
                &twi_core::executable_path(&root, &manifest.application)?,
                &root,
            )
            .map_err(display_error)?;
        }
        logger.record("Committing installation and cleaning the rollback copy.");
        transaction.commit()
    })();
    match install {
        Ok(warning) => {
            if let Some(warning) = warning {
                logger.record(&format!(
                    "WARNING: Cleanup will be retried on the next installer run: {warning}"
                ));
            }
            Ok(())
        }
        Err(error) => {
            logger.record(&format!(
                "Installation failed; restoring previous state: {error}"
            ));
            // Also stop a newly launched process if persisting the commit failed.
            // Never kill the old app for a validation/prerequisite failure in staging.
            if transaction::journal_path(&root)?.exists()
                && root.join(twi_core::INSTALL_METADATA_FILENAME).exists()
            {
                // Killing is only required after a switch; rollback itself checks
                // the persisted phase, so expose that state instead of assuming it.
                if transaction.switched() {
                    if let Err(stop_error) = process::find_and_kill_processes_from_directory(&root)
                    {
                        return Err(format!("{error}\nCannot stop the new application for rollback: {stop_error}. Rerun setup to recover."));
                    }
                }
            }
            match transaction.rollback(&mut restore) {
                Ok(()) => Err(format!("{error}\nThe previous installation state was preserved.")),
                Err(rollback) => Err(format!("{error}\nRecovery could not finish: {rollback}. The recovery journal and rollback copy were retained; rerun setup.")),
            }
        }
    }
}

#[cfg(windows)]
fn previous_installation(
    root: &std::path::Path,
    identifier: &str,
) -> Result<Option<twi_core::InstallMetadata>, String> {
    if !root.try_exists().map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let mut entries = std::fs::read_dir(root).map_err(|error| error.to_string())?;
    if entries
        .next()
        .transpose()
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Ok(None);
    }
    let metadata = twi_core::read_install_metadata(root).map_err(|error| {
        format!("Refusing to overwrite an installation without valid ownership metadata: {error}")
    })?;
    if metadata.app_id != identifier {
        return Err("The existing directory belongs to another application.".into());
    }
    Ok(Some(metadata))
}

#[cfg(windows)]
fn display_error(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[cfg(windows)]
struct Logger {
    file: Option<std::sync::Mutex<std::fs::File>>,
    path: Option<std::path::PathBuf>,
}
#[cfg(windows)]
impl Logger {
    fn new() -> Self {
        let log = tempfile::Builder::new()
            .prefix("twi-installer-")
            .suffix(".log")
            .tempfile()
            .and_then(|file| file.keep().map_err(|error| error.error));
        match log {
            Ok((file, path)) => Self {
                file: Some(std::sync::Mutex::new(file)),
                path: Some(path),
            },
            Err(_) => Self {
                file: None,
                path: None,
            },
        }
    }
    fn record(&self, message: &str) {
        use std::io::Write;
        if let Some(file) = &self.file {
            if let Ok(mut file) = file.lock() {
                let _ = writeln!(
                    file,
                    "[{}] {message}",
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
                );
                let _ = file.flush();
            }
        }
    }
    fn location(&self) -> String {
        match &self.path {
            Some(path) => format!("Diagnostic log: {}", path.display()),
            None => "A diagnostic log could not be created in the temporary directory.".into(),
        }
    }
}
