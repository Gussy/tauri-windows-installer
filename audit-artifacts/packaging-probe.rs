use std::{fs, path::PathBuf};

#[path = "/Users/gus/Development/tauri-windows-installer/bundler/src/plugin_config.rs"]
mod plugin_config;
mod webview2 {
    include!("/Users/gus/Development/tauri-windows-installer/bundler/src/webview2.rs");
    pub fn probe(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        download_webview2_evergreen_impl(url)
    }
}

fn put16(d: &mut [u8], p: usize, v: u16) { d[p..p+2].copy_from_slice(&v.to_le_bytes()); }
fn put32(d: &mut [u8], p: usize, v: u32) { d[p..p+4].copy_from_slice(&v.to_le_bytes()); }
fn put64(d: &mut [u8], p: usize, v: u64) { d[p..p+8].copy_from_slice(&v.to_le_bytes()); }
fn minimal_pe() -> Vec<u8> {
    let mut d = vec![0u8; 1024];
    put16(&mut d, 0, 0x5a4d); put32(&mut d, 0x3c, 0x80); put32(&mut d, 0x80, 0x4550);
    put16(&mut d, 0x84, 0x8664); put16(&mut d, 0x86, 1);
    put16(&mut d, 0x94, 240); put16(&mut d, 0x96, 0x22);
    let o = 0x98;
    put16(&mut d, o, 0x20b); put32(&mut d, o+4, 512);
    put32(&mut d, o+16, 0x1000); put32(&mut d, o+20, 0x1000);
    put64(&mut d, o+24, 0x140000000); put32(&mut d, o+32, 4096); put32(&mut d, o+36, 512);
    put16(&mut d, o+40, 6); put16(&mut d, o+48, 6);
    put32(&mut d, o+56, 8192); put32(&mut d, o+60, 512); put16(&mut d, o+68, 2);
    put64(&mut d, o+72, 1048576); put64(&mut d, o+80, 4096);
    put64(&mut d, o+88, 1048576); put64(&mut d, o+96, 4096); put32(&mut d, o+108, 16);
    let s = o+240;
    d[s..s+5].copy_from_slice(b".text");
    put32(&mut d, s+8, 1); put32(&mut d, s+12, 4096);
    put32(&mut d, s+16, 512); put32(&mut d, s+20, 512); put32(&mut d, s+36, 0x60000020);
    d[512] = 0xc3;
    d
}
fn opts(stub: Vec<u8>, app: PathBuf, out: PathBuf) -> twi_core::BundleOptions {
    twi_core::BundleOptions {
        setup_exe: stub, name: "Test".into(), title: "Test".into(), version: "1.2.3".into(),
        identifier: "com.example.test".into(), publisher: "Example".into(), app,
        main_exe: None, icon: None, webview2: None, sign_command: None, output_dir: out,
        desktop_shortcut: false, on_progress: None,
    }
}
struct Args { sign_command: Option<String> }
fn resolve_sign_command(
    args: &Args,
    plugin_config: &plugin_config::TauriWindowsInstaller,
    tauri_conf: &tauri::Config,
) -> Option<String> {
    // Priority: CLI flag > plugin config > tauri.conf.json bundle.windows.signCommand
    //
    // Tauri's sign_command uses %1 as a placeholder for the binary path.
    // Our library appends the path as the last argument instead.
    // If %1 appears in a non-final position, we can't safely rewrite it
    // (would change argument structure), so we strip it and let the library append.
    // If %1 doesn't appear, the command works as-is since the library appends the path.
    args.sign_command
        .clone()
        .or_else(|| plugin_config.sign_command.clone())
        .or_else(|| {
            tauri_conf
                .bundle
                .windows
                .sign_command
                .as_ref()
                .map(|sc| match sc {
                    tauri::utils::config::CustomSignCommandConfig::Command(cmd) => {
                        strip_percent1_placeholder(cmd)
                    }
                    tauri::utils::config::CustomSignCommandConfig::CommandWithOptions {
                        cmd,
                        args,
                    } => {
                        let mut parts = vec![cmd.clone()];
                        parts.extend(
                            args.iter()
                                .map(|a| strip_percent1_placeholder(a))
                                .filter(|a| !a.is_empty()),
                        );
                        parts.join(" ")
                    }
                })
        })
}

/// Strip the Tauri `%1` placeholder from a sign command string.
/// Returns the trimmed string with `%1` removed. If the entire arg is `%1`,
/// returns empty string (caller filters it out).
fn strip_percent1_placeholder(s: &str) -> String {
    s.replace("%1", "").trim().to_string()
}


fn main() {
    let scratch = PathBuf::from("/private/tmp/twi-bundler-audit");
    fs::create_dir_all(&scratch).unwrap();
    let mut image = editpe::Image::parse(minimal_pe()).unwrap();
    let mut r = editpe::ResourceDirectory::default();
    r.root_mut().insert(editpe::ResourceEntryName::ID(24), editpe::ResourceEntry::Table(editpe::ResourceTable::default()));
    let manifest = "<?xml version=\"1.0\"?><assembly manifestVersion=\"1.0\" xmlns=\"urn:schemas-microsoft-com:asm.v1\"><trustInfo><security><requestedPrivileges><requestedExecutionLevel level=\"asInvoker\"/></requestedPrivileges></security></trustInfo></assembly>";
    r.set_manifest(manifest).unwrap();
    image.set_resource_directory(r).unwrap();
    let stub = image.data().to_vec();
    let reparsed = editpe::Image::parse(stub.as_slice()).unwrap();
    let before = reparsed.resource_directory().unwrap().get_manifest().unwrap();
    let mut output = vec![];
    libsui::PortableExecutable::from(&stub).unwrap().write_resource("TWI_RESOURCE", b"x".to_vec()).unwrap().build(&mut output).unwrap();
    let output_image = editpe::Image::parse(output).unwrap();
    println!("MANIFEST LOSS: before={}, after={:?}", before.is_some(), output_image.resource_directory().unwrap().get_manifest().unwrap());
    fs::write(scratch.join("app.exe"), b"exe").unwrap();
    for id in ["", "..", "../outside", r"C:\Outside"] {
        let mut o = opts(stub.clone(), scratch.join("app.exe"), scratch.join("out"));
        o.identifier = id.into();
        println!("INVALID IDENTIFIER {:?}: {:?}", id, twi_core::bundle(o).map(|o| o.size));
    }
    let app_dir = scratch.join("files");
    fs::create_dir_all(&app_dir).unwrap();
    fs::write(app_dir.join("ok.exe"), b"exe").unwrap();
    fs::write(scratch.join("outside.exe"), b"not archived").unwrap();
    for main in ["../outside.exe", ".", ""] {
        let mut o = opts(stub.clone(), app_dir.clone(), scratch.join("out"));
        o.main_exe = Some(main.into());
        println!("INVALID MAIN EXE {:?}: {:?}", main, twi_core::bundle(o).map(|o| o.size));
    }
    for v in [serde_json::json!({}), serde_json::json!({"desktopShortcut":false}), serde_json::json!({"icon":"icon.png"})] {
        println!("PLUGIN CONFIG {:?}: {:?}", v, serde_json::from_value::<plugin_config::TauriWindowsInstaller>(v.clone()));
    }
    for cmd in [r"signtool sign /f C:\certs\test.pfx", r"C:\tools\sign.exe %1", "signtool sign /p pass word"] {
        println!("SIGN TOKENIZATION {:?}: {:?}", cmd, shell_words::split(cmd));
    }
    for id in ["", "../outside", r"C:\Outside"] {
        println!("TAURI IDENTIFIER {:?}: {:?}", id, serde_json::from_value::<tauri::Config>(serde_json::json!({"identifier":id})).map(|c| c.identifier));
    }
    for sign in [
        serde_json::json!({"cmd":r"C:\Program Files\Signer\sign.exe", "args":["--password", "two words", "%1", "--quiet"]}),
        serde_json::json!(r#"sign.exe --input "%1" --quiet"#),
        serde_json::json!("sign.exe --input=%1 --quiet"),
    ] {
        let conf: tauri::Config = serde_json::from_value(serde_json::json!({"identifier":"com.example.test","bundle":{"windows":{"signCommand":sign.clone()}}})).unwrap();
        let resolved = resolve_sign_command(&Args {sign_command:None}, &plugin_config::TauriWindowsInstaller::default(), &conf).unwrap();
        let mut argv = shell_words::split(&resolved).unwrap(); argv.push("output.exe".into());
        println!("TAURI SIGN {:?}: {:?}", sign, argv);
    }
    let cache = scratch.join("cache"); fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join(webview2::WEBVIEW2_EVERGREEN_EXE), []).unwrap();
    std::env::set_var("CACHE_DIR", &cache);
    println!("EMPTY WEBVIEW2 CACHE: {:?}", webview2::probe("https://unused.example.invalid").map(|d|d.len()));
    if std::env::args().any(|a|a=="http") {
        use std::io::{Read,Write};
        let cache_http = scratch.join("cache-http"); fs::create_dir_all(&cache_http).unwrap();
        let _ = fs::remove_file(cache_http.join(webview2::WEBVIEW2_EVERGREEN_EXE));
        std::env::set_var("CACHE_DIR", &cache_http);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/webview2", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream,_) = listener.accept().unwrap();
            let mut request=[0u8;4096]; let _=stream.read(&mut request).unwrap();
            stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 17\r\nConnection: close\r\n\r\n<html>oops</html>\n").unwrap();
        });
        println!("HTTP 503 WEBVIEW2: {:?}", webview2::probe(&url).map(|d|String::from_utf8_lossy(&d).to_string()));
        server.join().unwrap();
        println!("SECOND DOWNLOAD FROM DEAD SERVER: {:?}", webview2::probe(&url).map(|d|String::from_utf8_lossy(&d).to_string()));
    }
}
