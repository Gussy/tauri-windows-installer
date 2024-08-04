use anyhow::Result;
use winreg::enums::*;
use winreg::RegKey;

const UNINSTALL_STR: &'static str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

fn main() {
    println!("Uninstalling application...");

    let app_id = env!("BUNDLED_APP_ID").to_string();
    remove_uninstall_entry(&app_id).expect("Failed to remove uninstall entry");

    // TODO: Remove installation directory
}

fn remove_uninstall_entry(app_id: &str) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall_key = hkcu.create_subkey(UNINSTALL_STR)?.0;
    uninstall_key.delete_subkey(&app_id)?;
    Ok(())
}
