use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let config_path = "config.toml";
    let config_contents = fs::read_to_string(config_path).expect("Failed to read config file");
    let config: toml::Value =
        toml::from_str(&config_contents).expect("Failed to parse config file");

    let external_program = config["external"]["program"]
        .as_str()
        .expect("Failed to get external program filename");
    let external_path = config["external"]["path"]
        .as_str()
        .expect("Failed to get external program path");

    // Set environment variables for the program
    println!("cargo:rustc-env=BUNDLED_APP_NAME={}", external_program);

    // Copy the external program to the output directory
    let source_path = PathBuf::from(external_path).join(external_program);
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = PathBuf::from(&out_dir).join(external_program);
    fs::copy(&source_path, &dest_path).expect("Failed to copy external program");

    println!("cargo:rerun-if-changed={}", source_path.display());
    println!("cargo:rerun-if-changed={}", config_path);
}
