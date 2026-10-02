use crate::state::{CleanupState, WORKER_SCRIPT};
use anyhow::{anyhow, bail, Context, Result};
use std::{
    env, fs,
    io::Write,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Pid, Signal, System};
use twi_core::{
    acquire_install_lock, discover_installation, operation_lock_path, reject_reparse_point,
};
use winreg::{enums::*, RegKey, RegValue};
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

pub fn handle_uninstall() -> bool {
    if !env::args_os().any(|a| a == "--uninstall") {
        return false;
    }
    let silent = env::args_os().any(|a| a == "--silent");
    let result = prepare_uninstall();
    if let Err(error) = &result {
        let message = format!("Unable to uninstall: {error:#}\nRetry uninstall after closing applications. Recovery information has been retained.");
        eprintln!("{message}");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos());
        let log = env::temp_dir().join(format!("twi-uninstall-{}-{stamp}.log", std::process::id()));
        let saved = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&log)
            .and_then(|mut file| {
                file.write_all(message.as_bytes())?;
                file.sync_all()
            })
            .is_ok();
        let location = if saved {
            format!("Log: {}", log.display())
        } else {
            "No diagnostic log could be saved.".into()
        };
        if !silent {
            use windows::{
                core::HSTRING,
                Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
            };
            // SAFETY: owned UTF-16 strings live through this synchronous Win32 call.
            unsafe {
                MessageBoxW(
                    None,
                    &HSTRING::from(format!("{message}\n{location}")),
                    &HSTRING::from("Uninstall failed"),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
    }
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}
fn programs_dir() -> Result<PathBuf> {
    use windows::Win32::{
        System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED},
        UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG},
    };
    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }
    // An existing apartment is also acceptable; balance only successful calls.
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if initialized.is_err() && initialized.0 != 0x80010106u32 as i32 {
        initialized.ok()?;
    }
    let _apartment = Apartment(initialized.is_ok());
    // SAFETY: SDK folder ID; returned allocation is freed even if UTF-16 conversion fails.
    let value =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KNOWN_FOLDER_FLAG(0), None)? };
    let result = unsafe { value.to_hstring() };
    unsafe {
        CoTaskMemFree(Some(value.0.cast()));
    }
    Ok(PathBuf::from(result?.to_string_lossy()).join("Programs"))
}
fn powershell_path() -> Result<PathBuf> {
    let system = env::var_os("SystemRoot")
        .ok_or_else(|| anyhow!("Windows system directory is unavailable"))?;
    let path = powershell_path_in(Path::new(&system));
    if !path.is_file() {
        bail!("Windows PowerShell is unavailable");
    }
    Ok(path)
}
fn powershell_path_in(system: &Path) -> PathBuf {
    // PathBuf preserves separators inside pushed strings. Use separate components
    // so the serialized command exactly matches the worker's Windows command.
    system
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}
fn prepare_uninstall() -> Result<()> {
    let programs = programs_dir()?;
    let (_, metadata) =
        discover_installation(&env::current_exe()?, &programs).map_err(anyhow::Error::msg)?;
    // Keep the known-folder spelling in state; canonical Windows paths add a
    // device prefix which is still used for the OS lock identity.
    let root =
        twi_core::validate_install_root(&programs, &metadata.app_id).map_err(anyhow::Error::msg)?;
    let _lock = acquire_install_lock(&root).map_err(anyhow::Error::msg)?;
    // Discovery before the lock only selected its identity. An upgrade can
    // change executable layout and metadata before we acquire it.
    let (_, metadata) =
        discover_installation(&env::current_exe()?, &programs).map_err(anyhow::Error::msg)?;
    if programs.join(&metadata.app_id) != root {
        bail!("Installation ownership changed while waiting for the lock");
    }
    if programs
        .join(format!("{}.twi-transaction.json", metadata.app_id))
        .try_exists()?
    {
        bail!("An interrupted installation must be recovered by running setup first");
    }
    env::set_current_dir(&programs).context("Cannot move outside the installation directory")?;
    let marker = programs.join(format!("{}.twi-uninstall.json", metadata.app_id));
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let registry_path = format!("{UNINSTALL_KEY}\\{}", metadata.app_id);
    let prior_registry = registry_snapshot(&hkcu, &registry_path)?;
    let existing_recovery = marker.try_exists()?;
    let state = if existing_recovery {
        reject_reparse_point(&marker).map_err(anyhow::Error::msg)?;
        if fs::metadata(&marker)?.len() > twi_core::MAX_METADATA_SIZE as u64 {
            bail!("Uninstall recovery state is too large");
        }
        let mut state: CleanupState = serde_json::from_slice(&fs::read(&marker)?)?;
        state.validate(&programs).map_err(anyhow::Error::msg)?;
        state.parent_pid = std::process::id();
        state.parent_start = process_start(state.parent_pid)?;
        state
    } else {
        let workers = programs.join(".twi-uninstall-workers");
        reject_reparse_point(&workers).map_err(anyhow::Error::msg)?;
        fs::create_dir_all(&workers)?;
        let nonce = format!(
            "{:016x}{:016x}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64,
            u64::from(std::process::id())
        );
        let worker = workers.join(format!("{}-{nonce}", metadata.app_id));
        fs::create_dir(&worker)?;
        let mut script_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(worker.join("cleanup.ps1"))?;
        script_file.write_all(b"\xef\xbb\xbf")?;
        script_file.write_all(WORKER_SCRIPT.as_bytes())?;
        script_file.sync_all()?;
        let command = format!("\"{}\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\" -StatePath \"{}\"", powershell_path()?.display(), worker.join("cleanup.ps1").display(), marker.display());
        CleanupState {
            format_version: 1,
            retired: programs.join(format!("{}.twi-uninstall-{nonce}", metadata.app_id)),
            nonce,
            programs: programs.clone(),
            root: root.clone(),
            worker,
            lock: operation_lock_path(&root).map_err(anyhow::Error::msg)?,
            parent_pid: std::process::id(),
            parent_start: process_start(std::process::id())?,
            metadata,
            uninstall_command: command,
        }
    };
    state.validate(&programs).map_err(anyhow::Error::msg)?;
    let expected_command = format!(
        "\"{}\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\" -StatePath \"{}\"",
        powershell_path()?.display(),
        state.worker.join("cleanup.ps1").display(),
        marker.display()
    );
    if state.uninstall_command != expected_command {
        bail!("Invalid uninstall recovery command");
    }
    if prior_registry.is_some() {
        let key = hkcu.open_subkey(&registry_path)?;
        let location: String = key.get_value("InstallLocation")?;
        if fs::canonicalize(&location)? != fs::canonicalize(&root)? {
            bail!("Uninstall registration belongs to another installation");
        }
        let command: String = key.get_value("UninstallString")?;
        let app_command = format!(
            "\"{}\" --uninstall",
            twi_core::executable_path(&root, &state.metadata.app_exe)
                .map_err(anyhow::Error::msg)?
                .display()
        );
        if command != app_command && command != expected_command {
            bail!("Uninstall command belongs to another application");
        }
    }
    stop_application_processes(&root)?;
    atomic_state(&state.worker.join("state.json"), &state)?;
    atomic_state(&marker, &state)?;
    let result = (|| -> Result<()> {
        let (key, _) = hkcu.create_subkey(&registry_path)?;
        key.set_value("UninstallString", &state.uninstall_command)?;
        key.set_value("QuietUninstallString", &state.uninstall_command)?;
        key.set_value("InstallLocation", &root.to_string_lossy().as_ref())?;
        key.set_value("DisplayName", &state.metadata.app_title)?;
        key.set_value("DisplayVersion", &state.metadata.version)?;
        flush_registry(&key)?;
        let ready = state.worker.join("ready");
        reject_reparse_point(&ready).map_err(anyhow::Error::msg)?;
        match fs::remove_file(&ready) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let mut worker = Command::new(powershell_path()?)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(state.worker.join("cleanup.ps1"))
            .arg("-StatePath")
            .arg(&marker)
            .current_dir(&programs)
            .creation_flags(0x08000000)
            .spawn()
            .context("Cannot start cleanup worker")?;
        // The worker acknowledges validated state before waiting for this
        // process to exit. Script-policy failures leave the live app untouched.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if fs::read_to_string(&ready)
                .is_ok_and(|ack| ack == format!("{}:{}", state.nonce, state.parent_pid))
            {
                break;
            }
            if let Some(status) = worker.try_wait()? {
                bail!(
                    "Cleanup worker failed to initialize ({status}); see {}",
                    state.worker.join("cleanup.log").display()
                );
            }
            if Instant::now() >= deadline {
                let _ = worker.kill();
                let _ = worker.wait();
                bail!("Cleanup worker did not initialize within 15 seconds");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    if let Err(error) = result {
        restore_registry(&hkcu, &registry_path, prior_registry.as_ref()).context(format!(
            "{error:#}; restoring uninstall registration also failed"
        ))?;
        // A prior entry may already point to this worker. Never discard its
        // durable retry command when a later invocation cannot start it.
        if !existing_recovery {
            fs::remove_file(&marker)?;
            // Retain the worker's initialization diagnostics after restoring the
            // live application's registration. The error names cleanup.log.
        }
        return Err(error);
    }
    Ok(())
}
fn atomic_state(path: &Path, state: &CleanupState) -> Result<()> {
    let temporary = path.with_extension(format!(
        "json.{}-{}.tmp",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    reject_reparse_point(&temporary).map_err(anyhow::Error::msg)?;
    let bytes = serde_json::to_vec(state)?;
    if bytes.len() > twi_core::MAX_METADATA_SIZE {
        bail!("Uninstall recovery state is too large");
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    use windows::{
        core::HSTRING,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };
    // SAFETY: both owned path strings live through the synchronous Win32 call.
    unsafe {
        MoveFileExW(
            &HSTRING::from(temporary.as_os_str()),
            &HSTRING::from(path.as_os_str()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}
fn process_start(pid: u32) -> Result<u64> {
    let system = System::new_all();
    Ok(system
        .process(Pid::from_u32(pid))
        .ok_or_else(|| anyhow!("Cannot inspect uninstall process"))?
        .start_time())
}
fn stop_application_processes(root: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    let mut system = System::new_all();
    let mut targets = Vec::new();
    for (&pid, process) in system.processes() {
        if pid.as_u32() == std::process::id() {
            continue;
        }
        if process
            .exe()
            .and_then(|p| fs::canonicalize(p).ok())
            .is_some_and(|p| p.starts_with(&root))
        {
            if process.kill_with(Signal::Kill) != Some(true) {
                bail!("Cannot stop application process {pid}");
            }
            targets.push((pid, process.start_time()));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        system.refresh_all();
        if targets.iter().all(|(pid, start)| {
            system
                .process(*pid)
                .is_none_or(|p| p.start_time() != *start)
        }) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("Application processes did not stop within 15 seconds");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn registry_snapshot(hkcu: &RegKey, path: &str) -> Result<Option<Vec<(String, RegValue)>>> {
    match hkcu.open_subkey(path) {
        Ok(key) => Ok(Some(key.enum_values().collect::<std::io::Result<_>>()?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn restore_registry(
    hkcu: &RegKey,
    path: &str,
    values: Option<&Vec<(String, RegValue)>>,
) -> Result<()> {
    match hkcu.delete_subkey_all(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    if let Some(values) = values {
        let (key, _) = hkcu.create_subkey(path)?;
        for (name, value) in values {
            key.set_raw_value(name, value)?;
        }
        flush_registry(&key)?;
    }
    Ok(())
}

fn flush_registry(key: &RegKey) -> Result<()> {
    use windows::Win32::System::Registry::{RegFlushKey, HKEY};
    // SAFETY: RegKey owns this valid handle for the duration of the call.
    unsafe {
        RegFlushKey(HKEY(key.raw_handle() as *mut core::ffi::c_void)).ok()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_command_path_matches_worker_native_separators() {
        assert_eq!(
            powershell_path_in(Path::new(r"C:\Windows")),
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        );
        assert_eq!(
            powershell_path_in(Path::new(r"C:\Windows")).to_string_lossy(),
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
        );
    }
}
