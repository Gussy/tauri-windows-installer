mod application;
mod webview2;

pub(crate) use application::Application;
pub(crate) use webview2::WebView2;

use std::path::PathBuf;

pub(crate) trait Bundle {
    fn load() -> Self;
    fn is_installed() -> bool;
    fn install(&self, quiet: bool, path: &PathBuf) -> Result<(), anyhow::Error>;
}
