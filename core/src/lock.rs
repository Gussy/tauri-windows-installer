//! OS-backed operation lock shared by setup and uninstall.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

/// Closing the file releases the OS lock, including after process death.
pub struct InstallLock {
    _file: File,
}

/// The durable lock path is also used by the standalone uninstall worker.
pub fn operation_lock_path(root: &Path) -> Result<std::path::PathBuf, String> {
    let parent = root.parent().ok_or("Installation root has no parent")?;
    crate::reject_reparse_point(parent)?;
    let parent =
        fs::canonicalize(parent).map_err(|e| format!("Cannot resolve lock directory: {e}"))?;
    let name = root
        .file_name()
        .ok_or("Installation root has no filename")?;
    let identity = parent.join(name).to_string_lossy().to_lowercase();
    let key = format!("{:x}", Sha256::digest(identity.as_bytes()));
    let lock_dir = parent.join(".twi-locks");
    fs::create_dir_all(&lock_dir)
        .map_err(|e| format!("Cannot create operation lock directory: {e}"))?;
    crate::reject_reparse_point(&lock_dir)?;
    Ok(lock_dir.join(format!("{key}.lock")))
}

pub fn acquire_install_lock(root: &Path) -> Result<InstallLock, String> {
    let path = operation_lock_path(root)?;
    crate::reject_reparse_point(&path)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("Cannot open operation lock: {e}"))?;
    file.try_lock().map_err(|e| format!("Another installation or uninstall may be running, or the operation lock is unavailable: {e}"))?;
    Ok(InstallLock { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_is_exclusive_and_released_after_owner_drops() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("com.example.app");
        let lock = acquire_install_lock(&root).unwrap();
        assert!(acquire_install_lock(&root).is_err());
        let other = acquire_install_lock(&temporary.path().join("com.example.other")).unwrap();
        drop(other);
        drop(lock);
        assert!(acquire_install_lock(&root).is_ok());
    }
}
