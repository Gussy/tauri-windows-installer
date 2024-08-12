mod application;
mod webview2;

pub(crate) use application::Application;
pub(crate) use webview2::WebView2;

use bundler::SetupPackage;
use std::path::PathBuf;

pub(crate) trait Bundle {
    fn load(package: &SetupPackage) -> Self;
    fn is_installed() -> bool;
    fn install(&self, quiet: bool, path: &PathBuf) -> Result<(), anyhow::Error>;
}
