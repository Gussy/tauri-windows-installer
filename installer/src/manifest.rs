use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use tempdir::TempDir;
use winapi::shared::minwindef::{HGLOBAL, HRSRC};
use winapi::um::libloaderapi::{
    FindResourceW, LoadLibraryW, LoadResource, LockResource, SizeofResource,
};
use winapi::um::winuser::RT_MANIFEST;

pub fn extract_manifest_from_data(data: &[u8]) -> std::io::Result<()> {
    let temp_dir = TempDir::new("extract_manifest").expect("Failed to create temp dir");
    let temp_path = temp_dir.path().join("application.exe");

    {
        // Write the data to a temp file
        let mut temp_file = File::create(&temp_path).expect("Failed to create temp file");
        temp_file
            .write_all(data)
            .expect("Failed to write to temp file");
    }

    println!("Extracting manifest from {:?}", temp_path);
    extract_manifest(&temp_path)
}

fn extract_manifest(exe_path: &Path) -> std::io::Result<()> {
    unsafe {
        // Convert the path to a wide string
        let exe_path_wide: Vec<u16> = OsString::from(exe_path.as_os_str())
            .encode_wide()
            .chain(Some(0))
            .collect();

        // Load the executable module
        let h_module = LoadLibraryW(exe_path_wide.as_ptr());
        if h_module.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        // Find the manifest resource
        let h_res: HRSRC = FindResourceW(h_module, 1 as _, RT_MANIFEST);
        if h_res.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        // Load the manifest resource
        let h_global: HGLOBAL = LoadResource(h_module, h_res);
        if h_global.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        // Lock the resource to get a pointer to the data
        let p_data = LockResource(h_global);
        if p_data.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        // Get the size of the resource
        let size = SizeofResource(h_module, h_res);
        if size == 0 {
            return Err(std::io::Error::last_os_error());
        }
        println!("Manifest size: {}", size);

        // Write the manifest data to a file
        let data_slice = std::slice::from_raw_parts(p_data as *const u8, size as usize);
        let output_path = exe_path.with_extension("manifest.xml");
        let mut output_file = File::create(output_path)?;
        output_file.write_all(data_slice)?;
    }

    Ok(())
}
