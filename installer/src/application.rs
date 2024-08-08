use std::{fs, io::Write, path::PathBuf};

use crate::{process::spawn_detached_process, Bundle};
use anyhow::{Context, Result};
use bundler::SetupPackage;

pub struct Application {
    pub exe: String,
    pub data: Vec<u8>,
    pub size: u64,
}

impl Bundle for Application {
    fn load(package: &SetupPackage) -> Self {
        let data = package.get_application();

        Self {
            exe: package.manifest.application.clone(),
            data: data.clone(),
            size: data.clone().len() as u64,
        }
    }

    fn is_installed() -> bool {
        false
    }

    fn install(&self, _quiet: bool, path: &PathBuf) -> Result<()> {
        println!("Starting installation!");

        println!("Extracting application to installation directory...");
        let application_path = path.join(&self.exe);

        // Create and write to the application file
        let mut app_file = fs::File::create(&application_path).with_context(|| {
            format!(
                "Failed to create application file at {:?}",
                application_path
            )
        })?;
        app_file.write_all(&self.data).with_context(|| {
            format!("Failed to write application data to {:?}", application_path)
        })?;
        drop(app_file);

        // Start the application
        spawn_detached_process(application_path.clone())
            .with_context(|| format!("Failed to start application at {:?}", application_path))?;

        Ok(())
    }
}
