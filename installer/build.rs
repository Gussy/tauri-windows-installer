fn main() {
    // embed-manifest is only available as a build-dependency on Windows hosts
    // (via [target.'cfg(windows)'.build-dependencies] in Cargo.toml)
    #[cfg(windows)]
    {
        use embed_manifest::{embed_manifest, new_manifest};
        embed_manifest(new_manifest("app.manifest")).expect("unable to embed manifest file");
        println!("cargo:rerun-if-changed=app.manifest");
    }
}
