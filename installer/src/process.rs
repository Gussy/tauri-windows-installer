use anyhow::{anyhow, bail, Result};
use std::{fs, path::PathBuf, process::Command as Process, thread, time::Duration};
use sysinfo::Signal;
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

pub fn find_and_kill_processes_from_directory(
    directory: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let canonical_directory = fs::canonicalize(directory)?;
    let mut system = sysinfo::System::new_all();
    system.refresh_all();

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

    wait_for_processes_to_exit(&canonical_directory, Duration::from_secs(10))?;

    Ok(())
}

fn wait_for_processes_to_exit(
    canonical_directory: &std::path::Path,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now() + timeout;

    loop {
        let mut system = sysinfo::System::new_all();
        system.refresh_all();

        let remaining: Vec<_> = system
            .processes()
            .iter()
            .filter_map(|(&pid, process)| {
                if let Some(exe_path) = process.exe()?.to_str() {
                    if let Ok(canonical_path) = fs::canonicalize(exe_path) {
                        if canonical_path.starts_with(canonical_directory) {
                            return Some(pid);
                        }
                    }
                }
                None
            })
            .collect();

        if remaining.is_empty() {
            return Ok(());
        }

        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "Timed out waiting for {} process(es) in {} to exit",
                remaining.len(),
                canonical_directory.display()
            )
            .into());
        }

        println!(
            "Waiting for {} process(es) in {} to exit...",
            remaining.len(),
            canonical_directory.display()
        );
        thread::sleep(Duration::from_millis(200));
    }
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
    // SAFETY: child.id() returns a valid process ID from a just-spawned process.
    // AllowSetForegroundWindow is safe to call with any DWORD process ID.
    let _ = unsafe { AllowSetForegroundWindow(child.id()) };

    Ok(())
}
