#[cfg(target_os = "windows")]
mod windows_impl {
    use anyhow::Result;
    use std::env;
    use std::fs;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::Command as Process;
    use sysinfo::Signal;
    use winreg::enums::*;
    use winreg::RegKey;

    use twi_core::{InstallMetadata, INSTALL_METADATA_FILENAME};

    const UNINSTALL_STR: &'static str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

    /// Returns `true` if `--uninstall` was handled (caller should exit),
    /// `false` if the flag was not present (app continues normally).
    pub fn handle_uninstall() -> bool {
        let args: Vec<String> = std::env::args().collect();
        if args.contains(&"--uninstall".to_string()) {
            println!("Uninstall flag detected. Running uninstall code...");
            let exit_code = uninstall();
            std::process::exit(exit_code);
        }
        false
    }

    fn uninstall() -> i32 {
        let current_exe = env::current_exe().expect("Failed to get current executable path");
        let root_path = current_exe
            .parent()
            .expect("Failed to get parent directory")
            .to_path_buf();

        // Read install metadata
        let meta_path = root_path.join(INSTALL_METADATA_FILENAME);
        let metadata = match fs::read_to_string(&meta_path) {
            Ok(contents) => match serde_json::from_str::<InstallMetadata>(&contents) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("Failed to parse install metadata: {}", e);
                    return 1;
                }
            },
            Err(e) => {
                eprintln!(
                    "Failed to read install metadata from {:?}: {}",
                    meta_path, e
                );
                return 1;
            }
        };

        let mut errors = false;

        // Kill processes whose exe path is within the install directory
        if let Err(e) = kill_processes_in_directory(&root_path) {
            eprintln!("Failed to kill processes: {}", e);
            errors = true;
        }

        // Remove the installation directory contents (except for the executable itself)
        if let Err(e) = remove_dir_all_ext::remove_dir_but_not_self(&root_path) {
            eprintln!("Failed to remove installation directory: {}", e);
            errors = true;
        }

        // Remove desktop shortcut if one was created
        if metadata.desktop_shortcut {
            if let Err(e) = remove_desktop_shortcut(&metadata) {
                eprintln!("Failed to remove desktop shortcut: {}", e);
                errors = true;
            }
        }

        // Remove the uninstall registry key
        if let Err(e) = remove_uninstall_entry(&metadata.app_id) {
            eprintln!("Failed to remove uninstall registry key: {}", e);
            errors = true;
        }

        if errors {
            eprintln!("Uninstall completed with errors.");
        } else {
            println!("Uninstall completed successfully!");
        }

        // Delete the executable and its parent directory
        register_intent_to_delete_self(3, &root_path)
            .expect("Failed to register intent to delete self");

        errors as i32
    }

    fn kill_processes_in_directory(directory: &Path) -> Result<()> {
        let mut system = sysinfo::System::new_all();
        system.refresh_all();

        let canonical_directory = fs::canonicalize(directory)?;

        let processes: Vec<_> = system
            .processes()
            .iter()
            .filter_map(|(&pid, process)| {
                if let Some(exe_path) = process.exe()?.to_str() {
                    if let Ok(canonical_path) = fs::canonicalize(exe_path) {
                        if canonical_path.starts_with(&canonical_directory) {
                            // Don't kill ourselves
                            let current_pid = sysinfo::get_current_pid().ok()?;
                            if pid != current_pid {
                                return Some(pid);
                            }
                        }
                    }
                }
                None
            })
            .collect();

        println!(
            "Found {} processes in directory {:?}",
            processes.len(),
            directory
        );

        for pid in processes {
            if let Some(process) = system.process(pid) {
                println!("Terminating process {:?} with PID {}", process.name(), pid);
                process.kill_with(Signal::Kill);
            }
        }

        Ok(())
    }

    fn remove_desktop_shortcut(metadata: &InstallMetadata) -> Result<()> {
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath};

        // SAFETY: SHGetKnownFolderPath is a well-defined Win32 API. The FOLDERID_Desktop
        // pointer comes from a Windows SDK constant. The returned PWSTR is valid until freed.
        let desktop_path = unsafe {
            let flag = windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG(0);
            let result = SHGetKnownFolderPath(&FOLDERID_Desktop, flag, None)?;
            let hstring = result.to_hstring()?;
            hstring.to_string_lossy().trim_end_matches('\0').to_string()
        };

        let shortcut_path = Path::new(&desktop_path).join(format!("{}.lnk", metadata.app_title));
        match fs::remove_file(&shortcut_path) {
            Ok(()) => println!("Removed desktop shortcut: {}", shortcut_path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    fn remove_uninstall_entry(app_id: &str) -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let uninstall_key = hkcu.create_subkey(UNINSTALL_STR)?.0;
        uninstall_key.delete_subkey(&app_id)?;
        Ok(())
    }

    pub(crate) fn register_intent_to_delete_self(
        delay_seconds: usize,
        current_directory: &Path,
    ) -> Result<()> {
        println!("Deleting self...");
        let current_exe = env::current_exe()?.to_string_lossy().to_string();
        let dir_name = current_directory.file_name().unwrap().to_string_lossy();

        // Retry loop: wait, attempt delete, check if still exists, repeat up to 5 times
        // This handles cases where the OS holds a brief lock on the exe after process exit
        let command = format!(
            "for /L %i in (1,1,5) do (\
                choice /C Y /N /D Y /T {delay} & \
                Del /F /Q \"{exe}\" & \
                if not exist \"{exe}\" (\
                    Rmdir /S /Q \"{dir}\" & goto :eof\
                )\
            )",
            delay = delay_seconds,
            exe = current_exe,
            dir = dir_name,
        );
        println!("Running: cmd.exe /C {}", command);

        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Process::new("cmd.exe")
            .arg("/C")
            .raw_arg(command)
            .current_dir(current_directory.parent().unwrap())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;

        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::handle_uninstall;

#[cfg(not(target_os = "windows"))]
pub fn handle_uninstall() -> bool {
    false
}
