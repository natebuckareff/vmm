use std::{fs, path::PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::machine_id::MachineId;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub machines: Vec<Machine>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Machine {
    pub id: MachineId,
    pub name: String,
}

pub fn parse_config(path: &PathBuf) -> Result<Config> {
    let config_text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&config_text)?)
}

pub fn write_config(path: &PathBuf, config: Config) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}
