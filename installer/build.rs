fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=build.rs");
    // Build scripts execute on the host. Detect the compilation target rather
    // than cfg!(windows), so cross-built setup executables get the same manifest.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_manifest::embed_manifest_file("app.manifest")
            .expect("unable to embed the installer application manifest");
    }
}
