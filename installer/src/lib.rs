use anyhow::Result;
use winreg::enums::*;
use winreg::RegKey;

const UNINSTALL_STR: &'static str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

pub fn handle_uninstall(app_id: &str) {
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--uninstall".to_string()) {
        println!("Uninstall flag detected. Running uninstall code...");

        remove_uninstall_entry(&app_id).expect("Failed to remove uninstall entry");

        std::process::exit(0);
    }
}

fn remove_uninstall_entry(app_id: &str) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall_key = hkcu.create_subkey(UNINSTALL_STR)?.0;
    uninstall_key.delete_subkey(&app_id)?;
    Ok(())
}
