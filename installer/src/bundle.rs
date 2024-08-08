use std::path::PathBuf;

use bundler::SetupPackage;

pub trait Bundle {
    fn load(package: &SetupPackage) -> Self;
    fn is_installed() -> bool;
    fn install(&self, quiet: bool, path: &PathBuf) -> Result<(), anyhow::Error>;
}
