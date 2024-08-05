mod binary;
use crate::binary::Binary;

mod webview2;
use crate::webview2::Webview2;

fn main() {
    // Handle bundled external program
    let binary = Binary::load();
    println!("External program: {}", binary.name);

    // Handle bundled WebView2 runtime
    let webview2 = Webview2::load();
    println!("Webview2 bundled: {}", webview2.bundled);

    // Check if WebView2 runtime is installed
    let webview2_installed = webview2.installed;
    println!("Webview2 installed: {}", webview2_installed);
    if !webview2_installed {
        println!("Installing webview2 runtime...");
        webview2
            .install(false)
            .expect("Failed to install webview2 runtime");
    }

    // TODO:
    // - Extract and copy the bundled installer
    // - Set the windows registry keys
    // - Handle uninstall with command line flag
}
