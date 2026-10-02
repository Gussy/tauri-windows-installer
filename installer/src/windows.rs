use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use twi_core::SetupManifest;
use windows::{
    core::{Interface, GUID, PCWSTR, PWSTR},
    Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetVolumePathNameW},
    Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
    },
    Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_LocalAppData, IShellLinkW, SHGetKnownFolderPath, ShellLink,
    },
    Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
};
use winreg::{enums::*, RegKey, RegValue};

const UNINSTALL_STR: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
const SHORTCUT_LIMIT: u64 = 4 * 1024 * 1024;

struct ComApartment(bool);
impl ComApartment {
    fn initialize() -> Result<Self> {
        // This guard outlives interface objects, including on all error paths.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_ok() {
            return Ok(Self(true));
        }
        if result.0 == 0x80010106u32 as i32 {
            return Ok(Self(false));
        }
        result.ok()?;
        Ok(Self(false))
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}
struct KnownFolderBuffer(PWSTR);
impl Drop for KnownFolderBuffer {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0 .0.cast())) };
    }
}

pub fn get_local_app_data() -> Result<PathBuf> {
    get_known_folder(&FOLDERID_LocalAppData)
}

fn get_known_folder(folder: &GUID) -> Result<PathBuf> {
    let _com = ComApartment::initialize()?;
    let buffer = KnownFolderBuffer(unsafe {
        SHGetKnownFolderPath(
            folder,
            windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG(0),
            None,
        )
    }?);
    let value = unsafe { buffer.0.to_hstring() }?;
    Ok(PathBuf::from(OsString::from_wide(value.as_wide())))
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

pub fn show_error(message: &str) {
    let message = wide_string(message);
    let title = wide_string("Installation failed");
    // Strings are null-terminated and valid throughout this synchronous call.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        )
    };
}

pub fn write_uninstall_entry(
    manifest: &SetupManifest,
    root: &Path,
    installed_size: u64,
) -> Result<()> {
    let main =
        twi_core::executable_path(root, &manifest.application).map_err(anyhow::Error::msg)?;
    let command = format!("\"{}\" --uninstall", main.display());
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall = hkcu.create_subkey(UNINSTALL_STR)?.0;
    let key = uninstall.create_subkey(&manifest.identifier)?.0;
    key.set_value("DisplayIcon", &format!("\"{}\",0", main.display()))?;
    key.set_value("DisplayName", &manifest.title)?;
    key.set_value("DisplayVersion", &manifest.version)?;
    key.set_value("InstallDate", &Local::now().format("%Y%m%d").to_string())?;
    key.set_value("InstallLocation", &root.to_string_lossy().to_string())?;
    key.set_value("Publisher", &manifest.publisher)?;
    key.set_value("UninstallString", &command)?;
    key.set_value("QuietUninstallString", &format!("{command} --silent"))?;
    key.set_value(
        "EstimatedSize",
        &((installed_size / 1024).min(u32::MAX as u64) as u32),
    )?;
    key.set_value("NoModify", &1u32)?;
    key.set_value("NoRepair", &1u32)?;
    key.set_value("Language", &0x0409u32)?;
    flush_registry(&key)?;
    Ok(())
}

pub fn create_desktop_shortcut(manifest: &SetupManifest, root: &Path) -> Result<()> {
    let _com = ComApartment::initialize()?;
    let target = wide_path(
        &twi_core::executable_path(root, &manifest.application).map_err(anyhow::Error::msg)?,
    );
    let working = wide_path(root);
    let description = wide_string(&manifest.title);
    let temp = tempfile::Builder::new().prefix("twi-shortcut-").tempdir()?;
    let temporary = temp.path().join("shortcut.lnk");
    let temporary_wide = wide_path(&temporary);
    // All strings are valid through each call. Interface RAII drops before COM.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(PCWSTR(target.as_ptr()))?;
        link.SetWorkingDirectory(PCWSTR(working.as_ptr()))?;
        link.SetDescription(PCWSTR(description.as_ptr()))?;
        link.SetIconLocation(PCWSTR(target.as_ptr()), 0)?;
        let persistence: IPersistFile = link.cast()?;
        persistence.Save(PCWSTR(temporary_wide.as_ptr()), true)?;
    }
    let bytes =
        read_shortcut_bytes(&temporary)?.ok_or_else(|| anyhow!("The shortcut was not saved."))?;
    crate::transaction::atomic_write(&desktop_shortcut_path(&manifest.title)?, &bytes)
        .map_err(anyhow::Error::msg)
}

pub fn desktop_shortcut_exists(title: &str) -> Result<bool> {
    Ok(desktop_shortcut_path(title)?.try_exists()?)
}

pub fn shortcut_owned(title: &str, root: &Path, executable: &str) -> Result<bool> {
    let path = desktop_shortcut_path(title)?;
    if !path.try_exists()? {
        return Ok(false);
    }
    crate::transaction::reject_reparse_point(&path).map_err(anyhow::Error::msg)?;
    let _com = ComApartment::initialize()?;
    let wide = wide_path(&path);
    let expected = twi_core::executable_path(root, executable).map_err(anyhow::Error::msg)?;
    let mut target = vec![0u16; 32_768];
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let persistence: IPersistFile = link.cast()?;
        if persistence.Load(PCWSTR(wide.as_ptr()), STGM_READ).is_err()
            || link.GetPath(&mut target, std::ptr::null_mut(), 4).is_err()
        {
            return Ok(false);
        }
    }
    let end = target
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(target.len());
    let target = PathBuf::from(OsString::from_wide(&target[..end]));
    match (
        resolve_existing_ancestors(&target),
        resolve_existing_ancestors(&expected),
    ) {
        (Ok(target), Ok(expected)) => Ok(target
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())),
        _ => Ok(false),
    }
}

fn resolve_existing_ancestors(path: &Path) -> std::io::Result<PathBuf> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut canonical) => {
                for component in missing.into_iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(ancestor.file_name().ok_or(error)?);
                ancestor = ancestor.parent().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "No existing shortcut ancestor.",
                    )
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn desktop_shortcut_path(title: &str) -> Result<PathBuf> {
    validate_shortcut_title(title)?;
    Ok(get_known_folder(&FOLDERID_Desktop)?.join(format!("{title}.lnk")))
}
fn validate_shortcut_title(title: &str) -> Result<()> {
    twi_core::validate_windows_filename(title).map_err(anyhow::Error::msg)
}

pub fn remove_desktop_shortcut(title: &str) -> Result<()> {
    let path = desktop_shortcut_path(title)?;
    crate::transaction::reject_reparse_point(&path).map_err(anyhow::Error::msg)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredValue {
    name: String,
    kind: u32,
    bytes: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredShortcut {
    title: String,
    bytes: Option<Vec<u8>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationSnapshot {
    identifier: String,
    registry: Option<Vec<StoredValue>>,
    shortcuts: Vec<StoredShortcut>,
}

pub fn snapshot_registration(
    identifier: &str,
    root: &Path,
    titles: &[String],
) -> Result<serde_json::Value> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let registry = match hkcu.open_subkey(format!("{UNINSTALL_STR}\\{identifier}")) {
        Ok(key) => {
            let registered: String = key.get_value("InstallLocation").context(
                "The existing uninstall registration has no valid installation location",
            )?;
            let registered = resolve_existing_ancestors(Path::new(&registered))?;
            let expected = resolve_existing_ancestors(root)?;
            if !registered
                .to_string_lossy()
                .eq_ignore_ascii_case(&expected.to_string_lossy())
            {
                bail!("The existing uninstall registration belongs to a different installation location and will not be overwritten.");
            }
            if key.enum_keys().next().is_some() {
                bail!("The uninstall registry entry contains unexpected subkeys.");
            }
            Some(
                key.enum_values()
                    .map(|value| {
                        let (name, value) = value?;
                        Ok(StoredValue {
                            name,
                            kind: value.vtype as u32,
                            bytes: value.bytes,
                        })
                    })
                    .collect::<std::io::Result<Vec<_>>>()?,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let mut shortcuts = Vec::new();
    for title in titles {
        if shortcuts
            .iter()
            .any(|shortcut: &StoredShortcut| shortcut.title.eq_ignore_ascii_case(title))
        {
            continue;
        }
        shortcuts.push(StoredShortcut {
            title: title.clone(),
            bytes: read_shortcut_bytes(&desktop_shortcut_path(title)?)?,
        });
    }
    Ok(serde_json::to_value(RegistrationSnapshot {
        identifier: identifier.into(),
        registry,
        shortcuts,
    })?)
}

pub fn restore_registration(snapshot: &serde_json::Value, identifier: &str) -> Result<(), String> {
    let restore = || -> Result<()> {
        let snapshot: RegistrationSnapshot = serde_json::from_value(snapshot.clone())?;
        if snapshot.identifier != identifier || snapshot.shortcuts.len() > 2 {
            bail!("Recovery registration does not match this application.");
        }
        for shortcut in &snapshot.shortcuts {
            validate_shortcut_title(&shortcut.title)?;
            if shortcut
                .bytes
                .as_ref()
                .is_some_and(|bytes| bytes.len() as u64 > SHORTCUT_LIMIT)
            {
                bail!("Recovery shortcut is oversized.");
            }
        }
        if let Some(values) = &snapshot.registry {
            if values.len() > 512
                || values.iter().any(|value| {
                    value.kind > 11
                        || value.name.encode_utf16().count() > 16_383
                        || value.name.contains('\0')
                        || value.bytes.len() > 2 * 1024 * 1024
                })
            {
                bail!("Invalid or oversized recovery registry values.");
            }
        }
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let uninstall = hkcu.create_subkey(UNINSTALL_STR)?.0;
        match uninstall.delete_subkey(identifier) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Some(values) = snapshot.registry {
            let key = uninstall.create_subkey(identifier)?.0;
            for value in values {
                let kind = match value.kind {
                    0 => REG_NONE,
                    1 => REG_SZ,
                    2 => REG_EXPAND_SZ,
                    3 => REG_BINARY,
                    4 => REG_DWORD,
                    5 => REG_DWORD_BIG_ENDIAN,
                    6 => REG_LINK,
                    7 => REG_MULTI_SZ,
                    8 => REG_RESOURCE_LIST,
                    9 => REG_FULL_RESOURCE_DESCRIPTOR,
                    10 => REG_RESOURCE_REQUIREMENTS_LIST,
                    11 => REG_QWORD,
                    _ => bail!("Unsupported recovery registry type."),
                };
                key.set_raw_value(
                    &value.name,
                    &RegValue {
                        bytes: value.bytes,
                        vtype: kind,
                    },
                )?;
            }
            flush_registry(&key)?;
        }
        flush_registry(&uninstall)?;
        for shortcut in snapshot.shortcuts {
            let path = desktop_shortcut_path(&shortcut.title)?;
            match shortcut.bytes {
                Some(bytes) => {
                    crate::transaction::atomic_write(&path, &bytes).map_err(anyhow::Error::msg)?
                }
                None => remove_desktop_shortcut(&shortcut.title)?,
            }
        }
        Ok(())
    };
    restore().map_err(|error| format!("Cannot restore installation registration: {error:#}"))
}

fn read_shortcut_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    crate::transaction::reject_reparse_point(path).map_err(anyhow::Error::msg)?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(SHORTCUT_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > SHORTCUT_LIMIT {
        bail!("The desktop shortcut exceeds its supported size.");
    }
    Ok(Some(bytes))
}

pub fn get_free_space(path: &Path) -> Result<u64> {
    let mut available = 0;
    let path = wide_path(path);
    unsafe { GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut available), None, None) }
        .context("Cannot query storage available to the current user")?;
    Ok(available)
}

pub fn allocation_unit(path: &Path) -> Result<u64> {
    let path = wide_path(path);
    let mut volume = vec![0u16; 32_768];
    let mut sectors = 0;
    let mut bytes = 0;
    unsafe {
        GetVolumePathNameW(PCWSTR(path.as_ptr()), &mut volume)?;
        GetDiskFreeSpaceW(
            PCWSTR(volume.as_ptr()),
            Some(&mut sectors),
            Some(&mut bytes),
            None,
            None,
        )?;
    }
    let unit = sectors as u64 * bytes as u64;
    if unit == 0 {
        bail!("The filesystem returned an invalid allocation unit.");
    }
    Ok(unit)
}

fn flush_registry(key: &RegKey) -> Result<()> {
    use windows::Win32::System::Registry::{RegFlushKey, HKEY};
    // Borrow the live winreg handle; this call neither transfers nor closes it.
    unsafe { RegFlushKey(HKEY(key.raw_handle() as *mut _)) }.ok()?;
    Ok(())
}

pub fn check_os_version() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm.open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")?;
    let build: String = key.get_value("CurrentBuildNumber")?;
    let build: u32 = build
        .parse()
        .context("Cannot determine the Windows version")?;
    if build < 10240 {
        bail!("This installer requires Windows 10 or later.");
    }
    Ok(())
}
