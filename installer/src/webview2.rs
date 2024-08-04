use std::env;
use std::fs;

pub struct Webview2 {
    pub bundled: bool,
    pub data: Option<Vec<u8>>,
}

impl Webview2 {
    pub fn load() -> Self {
        let bundled = env!("WEBVIEW2_BUNDLED") == "true";
        if bundled {
            let name = env!("WEBVIEW2_BUNDLED_NAME").to_string();
            let data = Some(Self::load_data(&name));
            Self { bundled, data }
        } else {
            Self {
                bundled,
                data: None,
            }
        }
    }

    fn load_data(name: &str) -> Vec<u8> {
        let out_dir = env!("OUT_DIR");
        let binary_path = format!("{}/{}", out_dir, name);
        println!("Webview2 exe path: {}", binary_path);
        fs::read(&binary_path).expect("Failed to read external program binary")
    }
}
