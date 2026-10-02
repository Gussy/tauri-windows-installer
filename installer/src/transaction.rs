//! Recoverable directory replacement. All names are derived from the trusted root,
//! and the journal is flushed before any destructive operation.
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const JOURNAL_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Prepared,
    SwitchStarted,
    RollbackStarted,
    Committed,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    nonce: String,
    old_exists: bool,
    phase: Phase,
    external: serde_json::Value,
}

pub struct Transaction {
    root: PathBuf,
    journal: Journal,
}

impl Transaction {
    pub fn begin(root: &Path, external: serde_json::Value) -> Result<Self, String> {
        if journal_path(root)?.try_exists().map_err(io_error)? {
            return Err("An installation transaction still needs recovery.".into());
        }
        reject_reparse_point(root)?;
        let old_exists = root.try_exists().map_err(io_error)?;
        if old_exists && !root.is_dir() {
            return Err("The installation path is not a directory.".into());
        }
        let nonce = random_nonce();
        let mut transaction = Self {
            root: root.into(),
            journal: Journal {
                version: 1,
                nonce,
                old_exists,
                phase: Phase::Prepared,
                external,
            },
        };
        fs::create_dir(transaction.stage()?).map_err(io_error)?;
        if let Err(error) = transaction.persist() {
            let _ = remove_directory(&transaction.stage()?);
            return Err(error);
        }
        Ok(transaction)
    }

    pub fn stage(&self) -> Result<PathBuf, String> {
        sibling(&self.root, &format!(".twi-stage-{}", self.journal.nonce))
    }

    pub fn switched(&self) -> bool {
        self.journal.phase == Phase::SwitchStarted
            && self.stage().is_ok_and(|stage| !stage.exists())
    }

    fn backup(&self) -> Result<PathBuf, String> {
        sibling(&self.root, &format!(".twi-backup-{}", self.journal.nonce))
    }

    pub fn switch(&mut self) -> Result<(), String> {
        reject_reparse_point(&self.root)?;
        reject_reparse_point(&self.stage()?)?;
        reject_reparse_point(&self.backup()?)?;
        self.journal.phase = Phase::SwitchStarted;
        if let Err(error) = self.persist() {
            self.journal.phase = Phase::Prepared;
            return Err(error);
        }
        if self.journal.old_exists {
            durable_rename(&self.root, &self.backup()?)?;
        }
        durable_rename(&self.stage()?, &self.root)
    }

    /// The durable commit precedes cleanup. Cleanup errors are recoverable on the
    /// next run and must not cause a successful install to be rolled back.
    pub fn commit(&mut self) -> Result<Option<String>, String> {
        self.journal.phase = Phase::Committed;
        if let Err(error) = self.persist() {
            self.journal.phase = Phase::SwitchStarted;
            return Err(error);
        }
        Ok(self.cleanup().err())
    }

    pub fn rollback(
        &mut self,
        restore_external: &mut impl FnMut(&serde_json::Value) -> Result<(), String>,
    ) -> Result<(), String> {
        let stage = self.stage()?;
        let backup = self.backup()?;
        reject_reparse_point(&self.root)?;
        reject_reparse_point(&stage)?;
        reject_reparse_point(&backup)?;
        let backup_exists = backup.try_exists().map_err(io_error)?;
        let stage_exists = stage.try_exists().map_err(io_error)?;
        if self.journal.phase == Phase::SwitchStarted {
            if self.journal.old_exists && !backup_exists && !stage_exists {
                return Err(
                    "The rollback copy is missing; installation recovery requires manual review."
                        .into(),
                );
            }
            self.journal.phase = Phase::RollbackStarted;
            self.persist()?;
        }
        if self.journal.phase == Phase::RollbackStarted {
            if backup_exists {
                remove_directory(&self.root)?;
                durable_rename(&backup, &self.root)?;
            } else if !self.journal.old_exists && !stage_exists {
                remove_directory(&self.root)?;
            }
            restore_external(&self.journal.external)?;
        }
        self.cleanup()
    }

    fn cleanup(&mut self) -> Result<(), String> {
        if self.journal.phase == Phase::Committed && !self.committed_tree_present()? {
            return Err(
                "The committed installation tree is incomplete; the rollback copy was retained."
                    .into(),
            );
        }
        remove_directory(&self.stage()?)?;
        remove_directory(&self.backup()?)?;
        let path = journal_path(&self.root)?;
        reject_reparse_point(&path)?;
        fs::remove_file(&path).map_err(io_error)?;
        sync_parent(&path)
    }

    fn committed_tree_present(&self) -> Result<bool, String> {
        self.tree_is_valid(&self.root)
    }

    fn tree_is_valid(&self, tree: &Path) -> Result<bool, String> {
        reject_reparse_point(tree)?;
        if !tree.is_dir() {
            return Ok(false);
        }
        reject_reparse_point(&tree.join(twi_core::INSTALL_METADATA_FILENAME))?;
        let metadata = match twi_core::read_install_metadata(tree) {
            Ok(metadata) => metadata,
            Err(_) => return Ok(false),
        };
        if self.root.file_name().and_then(|name| name.to_str()) != Some(&metadata.app_id) {
            return Ok(false);
        }
        let mut executable = tree.to_path_buf();
        for component in metadata.app_exe.split(['/', '\\']) {
            executable.push(component);
            reject_reparse_point(&executable)?;
        }
        Ok(crate::executable::validate(&executable).is_ok())
    }

    fn persist(&mut self) -> Result<(), String> {
        let path = journal_path(&self.root)?;
        let bytes = serde_json::to_vec(&self.journal).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > JOURNAL_LIMIT {
            return Err("Installation recovery data exceeds its size limit.".into());
        }
        atomic_write(&path, &bytes)
    }
}

#[cfg(test)]
pub fn recover(
    root: &Path,
    restore_external: &mut impl FnMut(&serde_json::Value) -> Result<(), String>,
) -> Result<(), String> {
    recover_with_stop(root, restore_external, &mut |_| Ok(()))
}

pub fn recover_with_stop(
    root: &Path,
    restore_external: &mut impl FnMut(&serde_json::Value) -> Result<(), String>,
    stop_processes: &mut impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let path = journal_path(root)?;
    reject_reparse_point(&path)?;
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_error(e)),
    };
    let mut bytes = Vec::new();
    file.take(JOURNAL_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > JOURNAL_LIMIT {
        return Err("Installation journal exceeds its size limit.".into());
    }
    let journal: Journal = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Cannot read installation recovery journal: {e}"))?;
    if journal.version != 1
        || journal.nonce.len() != 24
        || !journal.nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Invalid installation recovery journal.".into());
    }
    let mut transaction = Transaction {
        root: root.into(),
        journal,
    };
    if transaction.journal.phase == Phase::Committed && !transaction.committed_tree_present()? {
        if !transaction.journal.old_exists || !transaction.tree_is_valid(&transaction.backup()?)? {
            return Err("The committed installation and rollback copy are unavailable or invalid. Both were retained; recovery requires manual review.".into());
        }
        transaction.journal.phase = Phase::SwitchStarted;
        transaction.persist()?;
    }
    if transaction.journal.phase == Phase::Committed {
        transaction.cleanup()
    } else {
        if matches!(
            transaction.journal.phase,
            Phase::SwitchStarted | Phase::RollbackStarted
        ) && (transaction.backup()?.exists()
            || (!transaction.journal.old_exists && !transaction.stage()?.exists()))
            && root.try_exists().map_err(io_error)?
        {
            stop_processes(root)?;
        }
        transaction.rollback(restore_external)
    }
}

pub fn journal_path(root: &Path) -> Result<PathBuf, String> {
    sibling(root, ".twi-transaction.json")
}

fn sibling(root: &Path, suffix: &str) -> Result<PathBuf, String> {
    let name = root
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid installation directory name.".to_string())?;
    let parent = root
        .parent()
        .ok_or_else(|| "Missing installation parent.".to_string())?;
    Ok(parent.join(format!("{name}{suffix}")))
}

fn random_nonce() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn reject_reparse_point(path: &Path) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_error(e)),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!("Refusing to follow a link at {}", path.display()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(format!(
                "Refusing to follow a reparse point at {}",
                path.display()
            ));
        }
    }
    Ok(())
}

pub fn remove_directory(path: &Path) -> Result<(), String> {
    reject_reparse_point(path)?;
    #[cfg(windows)]
    clear_readonly_files(path)?;
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Cannot remove {}: {e}", path.display())),
    }
}

#[cfg(windows)]
// On Windows this clears only FILE_ATTRIBUTE_READONLY; Unix permissions are
// never changed by this Windows-only cleanup helper.
#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly_files(root: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let mut paths = vec![root.to_path_buf()];
    while let Some(path) = paths.pop() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io_error(error)),
        };
        if metadata.file_attributes() & 0x400 != 0 {
            continue;
        }
        if metadata.permissions().readonly() {
            let mut permissions = metadata.permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&path, permissions).map_err(io_error)?;
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(path).map_err(io_error)? {
                paths.push(entry.map_err(io_error)?.path());
            }
        }
    }
    Ok(())
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    reject_reparse_point(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Missing file parent.".to_string())?;
    let temp = parent.join(format!(".twi-write-{}", random_nonce()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(io_error)?;
        file.write_all(bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        replace_file(&temp, path)?;
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn durable_rename(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.try_exists().map_err(io_error)? {
        return Err(format!(
            "The destination already exists: {}",
            destination.display()
        ));
    }
    #[cfg(windows)]
    move_file(source, destination, false)?;
    #[cfg(not(windows))]
    fs::rename(source, destination).map_err(io_error)?;
    sync_parent(destination)
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(windows)]
    move_file(source, destination, true)?;
    #[cfg(not(windows))]
    fs::rename(source, destination).map_err(io_error)?;
    Ok(())
}

#[cfg(windows)]
fn move_file(source: &Path, destination: &Path, replace: bool) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let flags = if replace {
        MOVEFILE_WRITE_THROUGH | MOVEFILE_REPLACE_EXISTING
    } else {
        MOVEFILE_WRITE_THROUGH
    };
    // Both strings are null-terminated and remain alive throughout the call.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let result =
            unsafe { MoveFileExW(PCWSTR(source.as_ptr()), PCWSTR(destination.as_ptr()), flags) };
        match result {
            Ok(()) => return Ok(()),
            Err(error) => {
                let code = error.code().0 as u32 & 0xffff;
                if !matches!(code, 5 | 32 | 33) || std::time::Instant::now() >= deadline {
                    return Err(format!(
                        "Cannot move installation file or directory: {error}"
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

fn sync_parent(path: &Path) -> Result<(), String> {
    #[cfg(not(windows))]
    File::open(path.parent().ok_or_else(|| "Missing parent.".to_string())?)
        .and_then(|directory| directory.sync_all())
        .map_err(io_error)?;
    #[cfg(windows)]
    let _ = path; // MoveFileExW uses WRITE_THROUGH; regular files are separately flushed.
    Ok(())
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("app");
        write_tree(&root, "old");
        (dir, root)
    }

    fn write_tree(root: &Path, version: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(root.join("version"), version).unwrap();
        fs::write(root.join("app.exe"), crate::executable::fixture_image()).unwrap();
        let metadata = twi_core::InstallMetadata {
            format_version: twi_core::FORMAT_VERSION,
            app_title: "Application".into(),
            app_id: "app".into(),
            app_exe: "app.exe".into(),
            version: "1.0.0".into(),
            desktop_shortcut: false,
            shortcut_title: None,
        };
        fs::write(
            root.join(twi_core::INSTALL_METADATA_FILENAME),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn staging_failure_keeps_live_install() {
        let (_dir, root) = fixture();
        let transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        fs::write(transaction.stage().unwrap().join("version"), "partial").unwrap();
        recover(&root, &mut |_| panic!("No external state was changed")).unwrap();
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
        assert!(!journal_path(&root).unwrap().exists());
    }

    #[test]
    fn recovery_after_switch_restores_files_and_registration() {
        let (_dir, root) = fixture();
        let mut transaction =
            Transaction::begin(&root, serde_json::json!({"registry":"old"})).unwrap();
        fs::write(transaction.stage().unwrap().join("version"), "new").unwrap();
        transaction.switch().unwrap();
        let mut restored = false;
        recover(&root, &mut |value| {
            assert_eq!(value["registry"], "old");
            restored = true;
            Ok(())
        })
        .unwrap();
        assert!(restored);
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
    }

    #[test]
    fn recovery_between_the_two_renames_restores_old_install() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        transaction.journal.phase = Phase::SwitchStarted;
        transaction.persist().unwrap();
        fs::rename(&root, transaction.backup().unwrap()).unwrap();
        recover(&root, &mut |_| Ok(())).unwrap();
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
    }

    #[test]
    fn failed_registration_restoration_can_be_retried() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        fs::write(transaction.stage().unwrap().join("version"), "new").unwrap();
        transaction.switch().unwrap();
        assert!(transaction
            .rollback(&mut |_| Err("registry denied".into()))
            .is_err());
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
        // Recovery needs a persistent indication that files have already rolled back.
        assert!(journal_path(&root).unwrap().exists());
        recover(&root, &mut |_| Ok(())).unwrap();
        assert!(!journal_path(&root).unwrap().exists());
    }

    #[test]
    fn committed_recovery_never_restores_the_old_version() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        write_tree(&transaction.stage().unwrap(), "new");
        transaction.switch().unwrap();
        transaction.journal.phase = Phase::Committed;
        transaction.persist().unwrap();
        recover(&root, &mut |_| {
            panic!("Committed registration must not be restored")
        })
        .unwrap();
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "new");
    }

    #[test]
    fn commit_removes_backups_and_marks_the_new_install_live() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        write_tree(&transaction.stage().unwrap(), "new");
        assert!(!transaction.switched());
        transaction.switch().unwrap();
        assert!(transaction.switched());
        assert!(transaction.commit().unwrap().is_none());
        assert!(!journal_path(&root).unwrap().exists());
        assert!(!transaction.backup().unwrap().exists());
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "new");
    }

    #[test]
    fn committed_journal_never_discards_the_only_remaining_tree() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        write_tree(&transaction.stage().unwrap(), "new");
        transaction.switch().unwrap();
        transaction.journal.phase = Phase::Committed;
        transaction.persist().unwrap();
        fs::remove_dir_all(&root).unwrap();
        let mut restored = false;
        recover(&root, &mut |_| {
            restored = true;
            Ok(())
        })
        .unwrap();
        assert!(restored);
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
    }

    #[test]
    fn committed_invalid_executable_recovers_the_valid_backup() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        write_tree(&transaction.stage().unwrap(), "new");
        transaction.switch().unwrap();
        transaction.journal.phase = Phase::Committed;
        transaction.persist().unwrap();
        fs::write(root.join("app.exe"), "corrupt").unwrap();
        recover(&root, &mut |_| Ok(())).unwrap();
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
        assert!(!journal_path(&root).unwrap().exists());
        // Recovery leaves a normal install, so a subsequent repair can proceed.
        Transaction::begin(&root, serde_json::Value::Null).unwrap();
        recover(&root, &mut |_| Ok(())).unwrap();
    }

    #[test]
    fn committed_invalid_trees_are_preserved_for_manual_recovery() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        write_tree(&transaction.stage().unwrap(), "new");
        transaction.switch().unwrap();
        transaction.journal.phase = Phase::Committed;
        transaction.persist().unwrap();
        fs::write(root.join("app.exe"), "corrupt").unwrap();
        fs::write(transaction.backup().unwrap().join("app.exe"), "corrupt").unwrap();
        assert!(recover(&root, &mut |_| panic!("No registration should change")).is_err());
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "new");
        assert_eq!(
            fs::read_to_string(transaction.backup().unwrap().join("version")).unwrap(),
            "old"
        );
        assert!(journal_path(&root).unwrap().exists());
    }

    #[test]
    fn recovery_stops_new_processes_before_removing_their_tree() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        fs::write(transaction.stage().unwrap().join("version"), "new").unwrap();
        transaction.switch().unwrap();
        let mut stopped = false;
        recover_with_stop(&root, &mut |_| Ok(()), &mut |path| {
            assert_eq!(fs::read_to_string(path.join("version")).unwrap(), "new");
            stopped = true;
            Ok(())
        })
        .unwrap();
        assert!(stopped);
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
    }

    #[test]
    fn corrupt_journal_cannot_delete_live_install() {
        let (_dir, root) = fixture();
        fs::write(
            journal_path(&root).unwrap(),
            br#"{"version":1,"nonce":"../../other"}"#,
        )
        .unwrap();
        assert!(recover(&root, &mut |_| Ok(())).is_err());
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
    }

    #[cfg(unix)]
    #[test]
    fn recovery_rejects_symlinked_backup() {
        let (_dir, root) = fixture();
        let mut transaction = Transaction::begin(&root, serde_json::Value::Null).unwrap();
        transaction.journal.phase = Phase::SwitchStarted;
        transaction.persist().unwrap();
        std::os::unix::fs::symlink(&root, transaction.backup().unwrap()).unwrap();
        assert!(recover(&root, &mut |_| Ok(())).is_err());
        assert_eq!(fs::read_to_string(root.join("version")).unwrap(), "old");
    }
}
