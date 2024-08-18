use std::env;
use std::fs;
use std::path::PathBuf;

const SETUP_EXE: &str = "setup.exe";
const SETUP_MISSING_ERR: &str = "setup.exe not found in manifest directory, please build the setup project:\n  cargo build --package twi_installer";

fn main() {
    // Only 64-bit is supported
    if env::var("CARGO_CFG_TARGET_ARCH").unwrap() != "x86_64" {
        panic!("Only 64-bit targets are supported");
    }

    // Get the manifest directory
    let manifest_dir_str = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set");
    let manifest_dir = PathBuf::from(manifest_dir_str);

    // // Get current target build directories
    let out_dir_str = env::var("OUT_DIR").expect("OUT_DIR environment variable not set");
    let out_dir = PathBuf::from(&out_dir_str);

    // If debug build, copy the locally built setup.exe to the manifest directory
    let profile = env::var("PROFILE").expect("PROFILE environment variable not set");
    if profile == "debug" {
        let target_dir = out_dir
            .ancestors()
            .nth(4) // Navigate up 4 levels
            .expect("Failed to get target directory")
            .join(&profile);

        let setup_source_path = target_dir.join(SETUP_EXE);
        let setup_dest_path = manifest_dir.join(SETUP_EXE);
        if !setup_source_path.exists() {
            panic!("{}", SETUP_MISSING_ERR);
        }

        if setup_dest_path.exists() {
            fs::remove_file(&setup_dest_path).expect("Failed to remove existing setup.exe");
        }
        fs::copy(&setup_source_path, &setup_dest_path).expect("Failed to copy setup.exe");
        println!("cargo:rerun-if-changed={}", setup_dest_path.display());
    }

    // Ensure the setup.exe is present in the manifest directory
    let setup_source_path = manifest_dir.join(SETUP_EXE);
    if !setup_source_path.exists() {
        panic!("{}", SETUP_MISSING_ERR);
    }

    // Copy the setup.exe for bundling
    let setup_dest_path = out_dir.join(SETUP_EXE);
    fs::copy(&setup_source_path, &setup_dest_path).expect("Failed to copy setup.exe");
    println!("cargo:rerun-if-changed={}", setup_source_path.display());
    println!("cargo:rustc-env=SETUP_EXE={}", SETUP_EXE);
}
