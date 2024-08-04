mod binary;
use crate::binary::Binary;

mod webview2;
use crate::webview2::Webview2;

fn main() {
    // Handle bundled external program
    let binary = Binary::load();
    println!("Binary data length: {}", binary.data.len());
    println!("External program: {}", binary.name);

    // Handle bundled WebView2 runtime
    let webview2 = Webview2::load();
    if webview2.bundled {
        println!("Webview2 bundled: true");
        println!("Webview2 data length: {}", webview2.data.unwrap().len());
    } else {
        println!("Webview2 bundled: false");
    }

    // TODO:
    // - Check for webview2 runtime
    // - Run webview2 installer if needed
    // - Extract and copy the bundled installer
    // - Set the windows registry keys
    // - Handle uninstall with command line flag
}
