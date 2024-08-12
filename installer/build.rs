use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    // Embed the app.manifest file
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("app.manifest")).expect("unable to embed manifest file");
        println!("cargo:rerun-if-changed=app.manifest");
    }
}
