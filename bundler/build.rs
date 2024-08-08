use std::env;
use std::fs;
use std::path::PathBuf;

const SETUP_EXE: &str = "setup.exe";

fn main() {
    // Get current target build directories
    let out_dir_str = env::var("OUT_DIR").expect("OUT_DIR environment variable not set");
    let profile = env::var("PROFILE").expect("PROFILE environment variable not set");
    let out_dir = PathBuf::from(&out_dir_str);
    let target_dir = out_dir
        .ancestors()
        .nth(4) // Navigate up 4 levels
        .expect("Failed to get target directory")
        .join(&profile);

    // Copy the setup.exe for bundling
    let setup_source_path = target_dir.join(SETUP_EXE);
    let setup_dest_path = out_dir.join(SETUP_EXE);
    fs::copy(&setup_source_path, &setup_dest_path).expect("Failed to copy setup.exe");
    println!("cargo:rerun-if-changed={}", setup_source_path.display());
    println!("cargo:rustc-env=SETUP_EXE={}", SETUP_EXE);
}
