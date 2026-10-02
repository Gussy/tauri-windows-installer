use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use twi_core::InstallMetadata;
pub const WORKER_SCRIPT: &str = include_str!("cleanup.ps1");
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupState {
    pub format_version: u32,
    pub nonce: String,
    pub programs: PathBuf,
    pub root: PathBuf,
    pub retired: PathBuf,
    pub worker: PathBuf,
    pub lock: PathBuf,
    pub parent_pid: u32,
    pub parent_start: u64,
    pub metadata: InstallMetadata,
    pub uninstall_command: String,
}

impl CleanupState {
    pub fn validate(&self, programs: &Path) -> Result<(), String> {
        twi_core::reject_reparse_point(programs)?;
        twi_core::reject_reparse_point(&programs.join(".twi-uninstall-workers"))?;
        self.metadata.validate()?;
        if self.format_version != 1
            || self.nonce.len() != 32
            || !self.nonce.bytes().all(|c| c.is_ascii_hexdigit())
            || self.programs != programs
            || self.root != programs.join(&self.metadata.app_id)
            || self.retired
                != programs.join(format!(
                    "{}.twi-uninstall-{}",
                    self.metadata.app_id, self.nonce
                ))
            || self.worker
                != programs
                    .join(".twi-uninstall-workers")
                    .join(format!("{}-{}", self.metadata.app_id, self.nonce))
            || self.lock != twi_core::operation_lock_path(&self.root)?
        {
            return Err("Invalid or incompatible uninstall recovery state".into());
        }
        for path in [&self.root, &self.retired, &self.worker, &self.lock] {
            twi_core::reject_reparse_point(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(programs: &Path) -> CleanupState {
        let nonce = "0123456789abcdef0123456789abcdef".to_string();
        let root = programs.join("com.example.app");
        CleanupState {
            format_version: 1,
            retired: programs.join(format!("com.example.app.twi-uninstall-{nonce}")),
            worker: programs
                .join(".twi-uninstall-workers")
                .join(format!("com.example.app-{nonce}")),
            lock: twi_core::operation_lock_path(&root).unwrap(),
            nonce,
            programs: programs.into(),
            root,
            parent_pid: 1,
            parent_start: 1,
            metadata: InstallMetadata {
                format_version: 1,
                app_title: "Example".into(),
                app_id: "com.example.app".into(),
                app_exe: "bin/app.exe".into(),
                version: "1.2.3".into(),
                desktop_shortcut: true,
                shortcut_title: Some("Old title".into()),
            },
            uninstall_command: "worker".into(),
        }
    }
    #[test]
    fn recovery_rejects_unowned_roots_and_unsafe_metadata() {
        let temporary = tempfile::tempdir().unwrap();
        let mut state = fixture(temporary.path());
        assert!(state.validate(temporary.path()).is_ok());
        state.retired = temporary.path().to_path_buf();
        assert!(state.validate(temporary.path()).is_err());
        state = fixture(temporary.path());
        state.metadata.app_id = "..".into();
        assert!(state.validate(temporary.path()).is_err());
        state = fixture(temporary.path());
        state.metadata.app_exe = "../../unrelated.exe".into();
        assert!(state.validate(temporary.path()).is_err());
        state = fixture(temporary.path());
        state.format_version = 2;
        assert!(state.validate(temporary.path()).is_err());
        assert!(!WORKER_SCRIPT.contains("Invoke-Expression"));
    }
    #[test]
    fn recovery_serialization_retains_nested_executable_and_shortcut_owner() {
        let temporary = tempfile::tempdir().unwrap();
        let state = fixture(temporary.path());
        let restored: CleanupState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        restored.validate(temporary.path()).unwrap();
        assert_eq!(restored.metadata.owned_shortcut_title(), Some("Old title"));
        assert_eq!(restored.metadata.app_exe, "bin/app.exe");
    }
}
