use crate::Bundle;
use anyhow::Result;
use std::env;
use std::fs;

pub struct Application {
    pub name: String,
    pub version: String,
    pub id: String,

    pub exe: String,
    pub data: Vec<u8>,
    pub size: u64,
}

impl Bundle for Application {
    fn load() -> Self {
        let name = env!("BUNDLED_APP_NAME").to_string();
        let version = env!("BUNDLED_APP_VERSION").to_string();
        let id = env!("BUNDLED_APP_ID").to_string();

        let exe = env!("BUNDLED_APP_EXE").to_string();
        let data = Self::load_data(&exe);

        Self {
            name,
            version,
            id,
            exe,
            data: data.clone(),
            size: data.clone().len() as u64,
        }
    }

    fn load_data(exe: &str) -> Vec<u8> {
        let out_dir = env!("OUT_DIR");
        let binary_path = format!("{}/{}", out_dir, exe);
        fs::read(&binary_path).expect("Failed to read external program binary")
    }

    fn is_installed() -> bool {
        false
    }

    fn install(&self, _quiet: bool) -> Result<()> {
        // Implement logic to install the binary
        Ok(())
    }
}
