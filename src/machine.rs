use std::{net::Ipv4Addr, path::PathBuf, process::Stdio};

use anyhow::{Context, Result, bail};
use byte_unit::Byte;
use futures::StreamExt;
use ipnet::Ipv4Net;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use url::Url;

use crate::{
    ctx::{HasDirs, HasLogger},
    id::Id,
    logger::{LogLine, LogSource, LogStream},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MachineConfig {
    pub name: String,
    pub cpus: u8,
    pub memory: Byte,
    pub image: Url,
    pub share_dirs: Vec<PathBuf>,
    pub user: MachineUserConfig,
    pub network: MachineNetworkConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MachineUserConfig {
    pub name: String,
    pub ssh_authorized_keys: Vec<String>,
}

impl MachineUserConfig {
    fn to_cloud_init_config(&self) -> Result<String> {
        use serde_yaml::{Mapping, Sequence, Value};

        let mut initial_user = Mapping::new();
        initial_user.insert(Value::from("name"), Value::from(self.name.clone()));
        initial_user.insert(
            Value::from("ssh_authorized_keys"),
            Value::from(self.ssh_authorized_keys.clone()),
        );

        let mut users = Sequence::new();
        users.push(Value::from(initial_user));

        let mut root = Mapping::new();
        root.insert(Value::from("users"), Value::from(users));

        let config_text =
            serde_yaml::to_string(&root).context("failed to serialize user cloud-init config")?;

        Ok(config_text)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MachineNetworkConfig {
    pub id: Id,
    pub interface: MachineInterfaceConfig,
}

impl MachineNetworkConfig {
    fn to_cloud_init_config(&self) -> Result<String> {
        match &self.interface {
            MachineInterfaceConfig::Static(config) => config.to_cloud_init_config(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum MachineInterfaceConfig {
    Static(MachineStaticNetworkConfig),
    // Dhcp(MachineDhcpNetworkConfig),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MachineStaticNetworkConfig {
    pub interface: String,
    pub ip: Ipv4Net,
    pub gateway: Ipv4Net,
    pub nameservers: Vec<Ipv4Addr>,
}

impl MachineStaticNetworkConfig {
    fn to_cloud_init_config(&self) -> Result<String> {
        use serde_yaml::{Mapping, Value};

        let mut interface = Mapping::new();
        interface.insert(Value::from("dhcp4"), Value::from("no"));
        interface.insert(
            Value::from("addresses"),
            Value::from(vec![self.ip.to_string()]),
        );
        interface.insert(
            Value::from("gateway4"),
            Value::from(self.gateway.to_string()),
        );
        interface.insert(
            Value::from("nameservers"),
            Value::from(
                self.nameservers
                    .iter()
                    .map(|ip| ip.to_string())
                    .collect::<Vec<_>>(),
            ),
        );

        let mut ethernets = Mapping::new();
        ethernets.insert(Value::from(self.interface.clone()), Value::from(interface));

        let mut network = Mapping::new();
        network.insert(Value::from("version"), Value::from(2));
        network.insert(Value::from("ethernets"), Value::from(ethernets));

        let mut root = Mapping::new();
        root.insert(Value::from("network"), Value::from(network));

        let config_text = serde_yaml::to_string(&root)
            .context("failed to serialize network cloud-init config")?;

        Ok(config_text)
    }
}

#[derive(Debug, Clone)]
pub struct Machine {
    id: Id,
    config: MachineConfig,
}

impl Machine {
    pub async fn new<Ctx: HasDirs>(ctx: &Ctx, id: Id, config: MachineConfig) -> Result<Self> {
        let config_path = ctx.dirs().get_machine_config_dir(id)?;
        if config_path.exists() {
            bail!("machine config exists: {}", config_path.display());
        }

        tokio::fs::create_dir_all(&config_path).await?;

        let config_file_path = config_path.join("config.json");

        let config_text = serde_json::to_string(&config)
            .context("failed to serialize machine config")
            .context(id)?;

        tokio::fs::write(config_file_path, config_text)
            .await
            .context("failed to write machine config")
            .context(id)?;

        Ok(Self { id, config })
    }

    pub async fn read<Ctx: HasDirs>(ctx: &Ctx, id: Id) -> Result<Self> {
        let config_path = ctx.dirs().get_machine_config_file_path(id)?;
        if !config_path.exists() || !config_path.is_file() {
            bail!("machine config file not found: {}", config_path.display());
        }

        let config_text = tokio::fs::read_to_string(config_path)
            .await
            .context("failed to read machine config")
            .context(id)?;

        let config: MachineConfig = serde_json::from_str(&config_text)
            .context("failed to parse machine config")
            .context(id)?;

        Ok(Self { id, config })
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn config(&self) -> &MachineConfig {
        &self.config
    }

    pub async fn get_root_image<Ctx: HasDirs + HasLogger>(&self, ctx: &Ctx) -> Result<PathBuf> {
        let config_path = ctx.dirs().get_machine_config_dir(self.id)?;
        let boot_image_path = config_path.join("root.qcow2");
        if boot_image_path.exists() {
            return Ok(boot_image_path);
        }

        let client = reqwest::Client::new();
        let response = client
            .get(self.config.image.clone())
            .send()
            .await
            .context("failed to download root image")
            .context(self.id)?;

        let status = response.status();
        if !status.is_success() {
            bail!("failed to download root image: {}", status);
        }

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(boot_image_path.clone())
            .await
            .context("failed to open root image")
            .context(self.id)?;

        let mut stream = response.bytes_stream();

        while let Some(chunk_result) = stream.next().await {
            // TODO: Should upgrade Logger to support progress events or add a
            // more generally purpose EventLogger for our own logs
            let chunk = chunk_result.context("failed to read chunk from response")?;
            file.write_all(&chunk)
                .await
                .context("failed to write chunk to file")?;
        }

        Ok(boot_image_path)
    }

    async fn write_cloud_init_config<Ctx: HasDirs>(&self, ctx: &Ctx) -> Result<()> {
        self.write_network_cloud_init_config(ctx).await?;
        self.write_user_cloud_init_config(ctx).await?;
        Ok(())
    }

    async fn write_network_cloud_init_config<Ctx: HasDirs>(&self, ctx: &Ctx) -> Result<()> {
        let config_path = ctx.dirs().get_machine_config_dir(self.id)?;
        tokio::fs::create_dir_all(&config_path).await?;

        let network_config_path = config_path.join("network-config.yaml");
        if network_config_path.exists() {
            return Ok(());
        }

        let network_config_text = self.config.network.to_cloud_init_config()?;

        let mut network_config_file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(network_config_path)
            .await
            .context("failed to open network cloud-init file")?;

        network_config_file
            .write_all("#cloud-config\n".as_bytes())
            .await
            .context("failed to write network cloud-init config")?;

        network_config_file
            .write_all(network_config_text.as_bytes())
            .await
            .context("failed to write network cloud-init config")?;

        Ok(())
    }

    async fn write_user_cloud_init_config<Ctx: HasDirs>(&self, ctx: &Ctx) -> Result<()> {
        let config_path = ctx.dirs().get_machine_config_dir(self.id)?;
        tokio::fs::create_dir_all(&config_path).await?;

        let user_config_path = config_path.join("user-config.yaml");
        if user_config_path.exists() {
            return Ok(());
        }

        let user_config_text = self.config.user.to_cloud_init_config()?;

        let mut user_config_file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(user_config_path)
            .await
            .context("failed to open user cloud-init file")?;

        user_config_file
            .write_all("#cloud-config\n".as_bytes())
            .await
            .context("failed to write user cloud-init config")?;

        user_config_file
            .write_all(user_config_text.as_bytes())
            .await
            .context("failed to write user cloud-init config")?;

        Ok(())
    }

    pub async fn get_cloud_init_iso<Ctx: HasDirs + HasLogger>(&self, ctx: &Ctx) -> Result<PathBuf> {
        let config_path = ctx.dirs().get_machine_config_dir(self.id)?;
        let cloud_init_iso_path = config_path.join("cloud-init.iso");
        if cloud_init_iso_path.exists() {
            println!(
                "using cached cloud-init.iso: {}",
                cloud_init_iso_path.display()
            );
            return Ok(cloud_init_iso_path);
        }

        self.write_cloud_init_config(ctx).await?;

        let args = vec![
            "-v",
            "cloud-init.iso",
            "--network=network-config.yaml",
            "user-config.yaml",
        ];

        let mut child = Command::new("cloud-localds")
            .args(args)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .current_dir(&config_path)
            .spawn()
            .context("failed to spawn cloud-localds")
            .context(self.id)?;

        let mut tasks = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let id = self.id.clone();
            let mut reader = BufReader::new(stdout).lines();
            let logger = ctx.logger().clone();
            let stdout_task = tokio::spawn(async move {
                while let Ok(Some(line)) = reader.next_line().await {
                    let _ = logger.log(LogLine::machine(
                        id,
                        LogStream::Stdout,
                        LogSource::CloudInit,
                        line,
                    ));
                }
            });
            tasks.push(stdout_task);
        }

        if let Some(stderr) = child.stderr.take() {
            let id = self.id.clone();
            let mut reader = BufReader::new(stderr).lines();
            let logger = ctx.logger().clone();
            let stderr_task = tokio::spawn(async move {
                while let Ok(Some(line)) = reader.next_line().await {
                    let _ = logger.log(LogLine::machine(
                        id,
                        LogStream::Stderr,
                        LogSource::CloudInit,
                        line,
                    ));
                }
            });
            tasks.push(stderr_task);
        }

        let status = child
            .wait()
            .await
            .context("failed to wait for cloud-localds")
            .context(self.id)?;

        for task in tasks.drain(..) {
            let _ = task.await;
        }

        if !status.success() {
            anyhow::bail!("cloud-localds exited with {}", status);
        }

        Ok(cloud_init_iso_path)
    }
}
