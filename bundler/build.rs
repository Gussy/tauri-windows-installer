use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=TWI_SETUP_EXE");
    println!("cargo:rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR missing"));
    let package =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory missing"));
    // Use Cargo's actual profile directory; PROFILE may only say release for a
    // custom profile, and --target / CARGO_TARGET_DIR change the directory tree.
    let built = out
        .ancestors()
        .nth(3)
        .expect("Cargo output directory layout changed")
        .join("setup.exe");
    let packaged = package.join("setup.exe");
    println!("cargo:rerun-if-changed={}", built.display());
    println!("cargo:rerun-if-changed={}", packaged.display());
    let explicit = env::var_os("TWI_SETUP_EXE").map(PathBuf::from);
    let source = explicit
        .clone()
        .or_else(|| built.is_file().then_some(built))
        .or_else(|| packaged.is_file().then_some(packaged));
    let bytes = if let Some(source) = source {
        println!("cargo:rerun-if-changed={}", source.display());
        let bytes = fs::read(&source)
            .unwrap_or_else(|e| panic!("Cannot read setup stub {}: {e}", source.display()));
        let offset = bytes
            .get(60..64)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()) as usize);
        let valid = bytes.starts_with(b"MZ")
            && offset
                .and_then(|o| bytes.get(o..o.saturating_add(6)))
                .is_some_and(|h| {
                    h.starts_with(b"PE\0\0") && &h[4..6] == 0x8664u16.to_le_bytes().as_slice()
                });
        assert!(
            valid,
            "Setup stub must be a valid Windows x86_64 PE executable"
        );
        assert!(
            bytes
                .windows(b"TWI_SETUP_ABI_V1\0".len())
                .any(|w| w == b"TWI_SETUP_ABI_V1\0"),
            "Setup stub is incompatible; rebuild twi_installer"
        );
        bytes
    } else {
        // Source-only builds and tests remain possible on all hosts. Running the
        // bundler then requires --setup-exe; an empty stub is never packaged.
        println!("cargo:warning=No setup stub embedded; pass --setup-exe at runtime or TWI_SETUP_EXE at build time");
        Vec::new()
    };
    fs::write(out.join("setup.exe"), &bytes).expect("Cannot stage embedded setup stub");
    println!("cargo:rustc-env=SETUP_EXE=setup.exe");
    println!(
        "cargo:rustc-env=TWI_SETUP_EXE_SHA256={:x}",
        Sha256::digest(&bytes)
    );
}
