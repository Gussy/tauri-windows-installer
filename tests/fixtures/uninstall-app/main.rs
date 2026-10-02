//! Small native fixture: stays alive so installation can prove PID replacement.
#![cfg_attr(windows, windows_subsystem = "windows")]
fn main() {
    if twi_uninstaller::handle_uninstall() {
        return;
    }
    let arguments: Vec<_> = std::env::args().collect();
    let _lock = if arguments.get(1).map(String::as_str) == Some("--test-hold-lock") {
        let root = std::path::Path::new(&arguments[2]);
        let lock = twi_core::acquire_install_lock(root).expect("fixture operation lock");
        std::fs::write(&arguments[3], b"locked").expect("fixture lock signal");
        Some(lock)
    } else {
        None
    };
    let executable = std::env::current_exe().expect("fixture executable");
    std::fs::write(
        executable.parent().unwrap().join(".fixture-cwd.txt"),
        std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .as_bytes(),
    )
    .expect("fixture cwd signal");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
