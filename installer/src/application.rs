use crate::Bundle;
use anyhow::Result;
use std::env;
use std::fs;

pub struct Application {
    pub name: String,
    pub id: String,
    pub data: Vec<u8>,
}

impl Bundle for Application {
    fn load() -> Self {
        let name = env!("BUNDLED_APP_NAME").to_string();
        let data = Self::load_data(&name);
        let id = env!("BUNDLED_APP_ID").to_string();
        Self { name, id, data }
    }

    fn load_data(name: &str) -> Vec<u8> {
        let out_dir = env!("OUT_DIR");
        let binary_path = format!("{}/{}", out_dir, name);
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
