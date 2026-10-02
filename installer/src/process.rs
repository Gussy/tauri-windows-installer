use anyhow::{anyhow, bail, Result};
use std::os::windows::process::CommandExt;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use sysinfo::Signal;
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

pub fn find_and_kill_processes_from_directory(directory: &Path) -> Result<()> {
    let directory = fs::canonicalize(directory)?;
    let current_pid = sysinfo::get_current_pid()
        .map_err(|error| anyhow!("Cannot determine installer PID: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let system = sysinfo::System::new_all();
        let processes: Vec<_> = system
            .processes()
            .iter()
            .filter_map(|(&pid, process)| {
                if pid == current_pid {
                    return None;
                }
                let path = fs::canonicalize(process.exe()?).ok()?;
                path.starts_with(&directory).then_some(pid)
            })
            .collect();
        if processes.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("Timed out stopping application processes; the existing installation was preserved.");
        }
        for pid in processes {
            if let Some(process) = system.process(pid) {
                if process.kill_with(Signal::Kill) != Some(true) {
                    // A process may have exited since enumeration. The next pass
                    // verifies that it disappeared before the directory is switched.
                    println!("Could not terminate process {pid}; checking again.");
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn spawn_detached_process(executable: &Path, working_directory: &Path) -> Result<()> {
    if !executable.is_file() {
        bail!("The installed application executable is missing.");
    }
    let child = Command::new(executable)
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0000_0008 | 0x0000_0200) // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        .spawn()
        .map_err(|error| anyhow!("Cannot start the installed application: {error}"))?;
    unsafe {
        let _ = AllowSetForegroundWindow(child.id());
    }
    Ok(())
}
