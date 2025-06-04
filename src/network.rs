use std::{process::ExitStatus, time::Duration};

use anyhow::{Context, Result, bail};
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::{ctx::HasDirs, id::Id, instance::Instance};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NetworkConfig {
    pub name: String,
    pub ip: Ipv4Net,
}

#[derive(Debug, Clone)]
pub struct Network {
    id: Id,
    config: NetworkConfig,
}

impl Network {
    pub async fn new<Ctx: HasDirs>(ctx: &Ctx, id: Id, config: NetworkConfig) -> Result<Self> {
        let config_path = ctx.dirs().get_network_config_dir(id)?;
        if config_path.exists() {
            bail!("network config exists: {}", config_path.display());
        }

        tokio::fs::create_dir_all(&config_path).await?;

        let config_file_path = config_path.join("config.json");

        let config_text = serde_json::to_string(&config)
            .context("failed to serialize network config")
            .context(id)?;

        tokio::fs::write(config_file_path, config_text)
            .await
            .context("failed to write network config")
            .context(id)?;

        Ok(Self { id, config })
    }

    pub async fn read<Ctx: HasDirs>(ctx: &Ctx, id: Id) -> Result<Self> {
        let config_path = ctx.dirs().get_network_config_file_path(id)?;
        if !config_path.exists() || !config_path.is_file() {
            bail!("network config file not found: {}", config_path.display());
        }

        let config_text = tokio::fs::read_to_string(config_path)
            .await
            .context("failed to read network config")
            .context(id)?;

        let config: NetworkConfig = serde_json::from_str(&config_text)
            .context("failed to parse network config")
            .context(id)?;

        Ok(Self { id, config })
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn config(&self) -> &NetworkConfig {
        &self.config
    }

    pub async fn set_bridge_up_or_create(&self) -> Result<()> {
        let bridge = self.get_bridge_name();

        // TODO: can set and check a flag instead to speed up calling this many
        // times in sequence

        if !cmd("ip", &["link", "show", &bridge]).await?.success() {
            cmd_success("ip", &["link", "add", &bridge, "type", "bridge"]).await?;

            loop {
                let ret = cmd("ip", &["link", "show", &bridge]).await?;
                if ret.success() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        cmd_success(
            "ip",
            &["addr", "add", &self.config.ip.to_string(), "dev", &bridge],
        )
        .await?;

        cmd_success("ip", &["link", "set", "up", "dev", &bridge]).await?;

        Ok(())
    }

    pub async fn set_tap_up_or_create(&self, instance: &Instance) -> Result<()> {
        let bridge = self.get_bridge_name();
        let tap = self.get_tap_name(instance);

        // TODO: can set and check a flag instead to speed up calling this many
        // times in sequence

        if !cmd("ip", &["link", "show", &tap]).await?.success() {
            cmd_success("ip", &["tuntap", "add", &tap, "mode", "tap"]).await?;

            loop {
                let ret = cmd("ip", &["link", "show", &tap]).await?;
                if ret.success() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        cmd_success("ip", &["link", "set", &tap, "up"]).await?;
        cmd_success("ip", &["link", "set", &tap, "master", &bridge]).await?;

        Ok(())
    }

    async fn delete_tap_device(&self, instance: &Instance) -> Result<()> {
        let tap = self.get_tap_name(instance);
        cmd_success("ip", &["link", "set", &tap, "down"]).await?;
        cmd_success("ip", &["link", "delete", &tap]).await?;
        Ok(())
    }

    async fn delete_bridge_device(&self) -> Result<()> {
        let name = self.get_bridge_name();
        cmd_success("ip", &["link", "set", &name, "down"]).await?;
        cmd_success("ip", &["link", "delete", &name]).await?;
        Ok(())
    }

    pub fn get_bridge_name(&self) -> String {
        let id = self.id.to_string();
        let id = &id[id.len() - 4..];
        format!("vmmbr-{}", id)
    }

    pub fn get_tap_name(&self, instance: &Instance) -> String {
        let id = instance.id().to_string();
        let id = &id[id.len() - 4..];
        format!("vmmtap-{}", id)
    }
}

// TODO: move to cmd.rs?
async fn cmd(cmd: &str, args: &[&str]) -> Result<ExitStatus> {
    let ecode = Command::new(cmd).args(args).spawn()?.wait().await?;
    Ok(ecode)
}

// TODO: move to cmd.rs?
async fn cmd_success(cmd: &str, args: &[&str]) -> Result<ExitStatus> {
    let ecode = Command::new(cmd).args(args).spawn()?.wait().await?;
    if !ecode.success() {
        bail!("command failed: {}", cmd)
    }
    Ok(ecode)
}
