//! Per-user uninstall with a durable, independent cleanup worker.
//! The worker remains registered until removal succeeds, so locked files can be retried.
#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::handle_uninstall;
#[cfg(not(windows))]
pub fn handle_uninstall() -> bool {
    false
}
#[cfg(any(windows, test))]
mod state;
