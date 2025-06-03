use std::{fs, path::PathBuf};

use anyhow::Result;
use byte_unit::Byte;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};

use crate::id::Id;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub machines: Vec<Machine>,
    pub networks: Vec<Network>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Machine {
    pub id: Id,
    pub name: String,
    pub network: Id,
    pub cpus: u8,
    pub memory: Byte,
    pub iso: PathBuf,
    pub boot: PathBuf,
    pub virtiofs: Vec<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Network {
    pub id: Id,
    pub name: String,
    pub ip: Ipv4Net,
}

pub fn parse_config(path: &PathBuf) -> Result<Config> {
    let config_text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&config_text)?)
}

pub fn write_config(path: &PathBuf, config: Config) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}
