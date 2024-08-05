use std::fs;

use sysinfo::Signal;

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
