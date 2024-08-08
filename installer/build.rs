use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    // Embed the app.manifest file
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("app.manifest")).expect("unable to embed manifest file");
        println!("cargo:rerun-if-changed=app.manifest");
    }
}

// pub fn build_resource() {
//     #[cfg(bin)]
//     if Ok("release".to_owned()) == std::env::var("PROFILE") {
//         let mut res = winres::WindowsResource::new();
//         res.set_icon("../libs/myicon.ico");
//         if let Err(e) = res.compile() {
//             eprintln!("{}", e);
//             std::process::exit(1);
//         }
//     }
// }

// #[cfg(target_os = "windows")]
//     winres::WindowsResource::new()
//         .set_manifest_file("app.manifest")
//         .set_version_info(winres::VersionInfo::PRODUCTVERSION, ver)
//         .set_version_info(winres::VersionInfo::FILEVERSION, ver)
//         .set("CompanyName", "Velopack")
//         .set("ProductName", "Velopack")
//         .set("ProductVersion", &version)
//         .set("FileDescription", &desc)
//         .set("LegalCopyright", "Caelan Sayler (c) 2023, Velopack Ltd. (c) 2024")
//         .compile()
//         .unwrap();
