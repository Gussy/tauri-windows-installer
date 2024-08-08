use anyhow::{anyhow, bail, Result};
use std::{fs, path::PathBuf, process::Command as Process};
use sysinfo::Signal;
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

pub fn find_and_kill_processes_from_directory(
    directory: &str,
) -> Result<(), Box<dyn std::error::Error>> {
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
                        return Some(pid);
                    }
                }
            }
            None
        })
        .collect();
    println!(
        "Found {} processes in directory {}",
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

pub fn spawn_detached_process(exe_path: PathBuf) -> Result<()> {
    let exe_to_execute = std::path::Path::new(&exe_path);
    if !exe_to_execute.exists() {
        bail!(
            "Unable to find executable to start: '{}'",
            exe_to_execute.to_string_lossy()
        );
    }

    let mut exe_launch = Process::new(&exe_to_execute);

    println!("About to launch: '{}'", exe_to_execute.to_string_lossy());
    let child = exe_launch
        .spawn()
        .map_err(|z| anyhow!("Failed to start application ({}).", z))?;
    let _ = unsafe { AllowSetForegroundWindow(child.id()) };

    Ok(())
}
