use serde_json::Error as SerdeError;
use std::fs;
use std::io::Error as IoError;

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct TauriWindowsInstaller {
    pub icon: Option<String>,
    pub webview2: Webview2Config,
}

#[derive(Debug, Deserialize, Default)]
pub struct Webview2Config {
    pub bundle: Option<Webview2Bundle>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Webview2Bundle {
    Evergreen,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(IoError),
    Serde(SerdeError),
}

impl From<IoError> for ConfigError {
    fn from(err: IoError) -> Self {
        ConfigError::Io(err)
    }
}

impl From<SerdeError> for ConfigError {
    fn from(err: SerdeError) -> Self {
        ConfigError::Serde(err)
    }
}

pub fn load_tauri_config(
    tauri_conf_path: &str,
) -> Result<(tauri::Config, TauriWindowsInstaller), ConfigError> {
    // Read the configuration file from the given path
    let tauri_conf_contents = fs::read_to_string(tauri_conf_path)?;

    // Parse the Tauri configuration from the JSON content
    let tauri_conf: tauri::Config = serde_json::from_str(&tauri_conf_contents)?;

    // Parse the plugin config or use the default if it's not present
    let plugin_config =
        if let Some(plugin_value) = tauri_conf.plugins.0.get("tauri-windows-installer") {
            serde_json::from_value(plugin_value.clone())?
        } else {
            TauriWindowsInstaller::default()
        };

    // Return the parsed configuration and plugin config as a tuple
    Ok((tauri_conf, plugin_config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_load_tauri_config_with_plugin() {
        let config_json = json!({
            "$schema": null,
            "productName": "test-app",
            "version": "0.0.0",
            "identifier": "com.example.test",
            "app": {},
            "build": {},
            "bundle": {},
            "plugins": {
                "tauri-windows-installer": {
                    "icon": "icons/icon.ico",
                    "webview2": {
                        "bundle": "evergreen"
                    }
                }
            }
        });

        let temp_dir = std::env::temp_dir();
        let config_path = temp_dir.join("tauri_config_with_plugin.json");

        let mut file = File::create(&config_path).expect("Failed to create test config file");
        file.write_all(config_json.to_string().as_bytes())
            .expect("Failed to write to test config file");

        let (_tauri_conf, plugin_config) =
            load_tauri_config(config_path.to_str().unwrap()).expect("Failed to load tauri config");

        assert_eq!(plugin_config.icon, Some("icons/icon.ico".to_string()));
        assert_eq!(
            plugin_config.webview2.bundle,
            Some(Webview2Bundle::Evergreen)
        );
    }

    #[test]
    fn test_load_tauri_config_without_plugin() {
        let config_json = json!({
            "$schema": null,
            "productName": "test-app",
            "version": "0.0.0",
            "identifier": "com.example.test",
            "app": {},
            "build": {},
            "bundle": {},
            "plugins": {}
        });

        let temp_dir = std::env::temp_dir();
        let config_path = temp_dir.join("tauri_config_without_plugin.json");

        let mut file = File::create(&config_path).expect("Failed to create test config file");
        file.write_all(config_json.to_string().as_bytes())
            .expect("Failed to write to test config file");

        let (_tauri_conf, plugin_config) =
            load_tauri_config(config_path.to_str().unwrap()).expect("Failed to load tauri config");

        assert_eq!(plugin_config.icon, None);
        assert_eq!(plugin_config.webview2.bundle, None);
    }
}
