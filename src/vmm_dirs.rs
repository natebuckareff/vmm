use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow};
use directories::BaseDirs;

use crate::id::Id;

#[derive(Debug, Clone)]
pub struct VmmDirs {
    base_dirs: BaseDirs,
    config_dir: PathBuf,
    cache_dir: PathBuf,
    state_dir: PathBuf,
}

impl VmmDirs {
    pub fn new() -> Result<Self> {
        let base_dirs = BaseDirs::new().ok_or(anyhow!("no base dirs"))?;
        let config_dir = base_dirs.config_dir().join("vmm");
        let cache_dir = base_dirs.cache_dir().join("vmm");
        let state_dir = base_dirs
            .state_dir()
            .ok_or(anyhow!("no state dir"))?
            .join("vmm");

        Ok(Self {
            base_dirs,
            config_dir,
            cache_dir,
            state_dir,
        })
    }

    pub fn get_network_config_dir(&self, network_id: Id) -> Result<PathBuf> {
        let path = self
            .config_dir
            .join("networks")
            .join(network_id.to_string());
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn get_network_config_file_path(&self, id: Id) -> Result<PathBuf> {
        let config_path = self.get_network_config_dir(id)?.join("config.json");
        Ok(config_path)
    }

    pub fn get_machine_config_dir(&self, machine_id: Id) -> Result<PathBuf> {
        let path = self
            .config_dir
            .join("machines")
            .join(machine_id.to_string());
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn get_machine_config_file_path(&self, id: Id) -> Result<PathBuf> {
        let config_path = self.get_machine_config_dir(id)?.join("config.json");
        Ok(config_path)
    }

    pub fn get_machine_cache_dir(&self, machine_id: Id) -> Result<PathBuf> {
        let path = self.cache_dir.join("machines").join(machine_id.to_string());
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn get_machine_log_dir(&self, machine_id: Id) -> Result<PathBuf> {
        let path = self
            .state_dir
            .join("machines")
            .join(machine_id.to_string())
            .join("logs");
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn get_instance_log_dir(&self, instance_id: Id) -> Result<PathBuf> {
        let path = self
            .state_dir
            .join("instances")
            .join(instance_id.to_string())
            .join("logs");
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}
