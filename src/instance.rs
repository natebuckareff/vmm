use std::process::Stdio;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
};

use crate::{
    ctx::{HasDirs, HasLogger},
    id::Id,
    logger::{LogLine, LogSource, LogStream},
    machine::Machine,
    network::Network,
    share_dir::ShareDir,
};

#[derive(Debug, Serialize, Deserialize)]
struct InstanceState {
    id: Id,
    boot_seq: u64,
    machine_id: Id,
    network_id: Id,
}

pub struct Instance {
    id: Id,
    boot_seq: u64,
    machine: Machine,
    network: Network,
    share_dirs: Vec<ShareDir>,
    qemu: Option<(Child, Vec<JoinHandle<()>>)>,
}

impl Instance {
    pub async fn new<Ctx: HasDirs>(
        ctx: &Ctx,
        id: Id,
        machine: Machine,
        network: Network,
    ) -> Result<Self> {
        let state = InstanceState {
            id,
            boot_seq: 0,
            machine_id: machine.id().clone(),
            network_id: network.id().clone(),
        };

        let instance_state_path = ctx.dirs().get_instance_state_file_path(id)?;

        if instance_state_path.exists() {
            bail!(
                "instance state file already exists: {}",
                instance_state_path.display()
            );
        }

        let state_text = serde_json::to_string(&state)
            .context("failed to serialize instance state")
            .context(id)?;

        tokio::fs::write(instance_state_path, state_text)
            .await
            .context("failed to write instance state")
            .context(id)?;

        let share_dirs = Self::init_share_dirs(&machine, id, 0)?;

        Ok(Self {
            id,
            boot_seq: 0,
            machine,
            network,
            share_dirs,
            qemu: None,
        })
    }

    pub async fn read<Ctx: HasDirs>(ctx: &Ctx, id: Id) -> Result<Self> {
        let instance_state_path = ctx.dirs().get_instance_state_file_path(id)?;

        if !instance_state_path.exists() {
            bail!(
                "instance state file not found: {}",
                instance_state_path.display()
            );
        }

        let state_text = tokio::fs::read_to_string(&instance_state_path)
            .await
            .context("failed to read instance state")
            .context(id)?;

        let mut state: InstanceState = serde_json::from_str(&state_text)
            .context("failed to parse instance state")
            .context(id)?;

        state.boot_seq += 1;
        let boot_seq = state.boot_seq;

        let state_text = serde_json::to_string(&state)
            .context("failed to serialize instance state")
            .context(id)?;

        tokio::fs::write(instance_state_path, state_text)
            .await
            .context("failed to write instance state")
            .context(id)?;

        let machine = Machine::read(ctx, state.machine_id)
            .await
            .context("failed to read instance machine")
            .context(id)?;

        let network = Network::read(ctx, state.network_id)
            .await
            .context("failed to read instance network")
            .context(id)?;

        let share_dirs = Self::init_share_dirs(&machine, id, boot_seq)?;

        Ok(Self {
            id,
            boot_seq,
            machine,
            network,
            share_dirs,
            qemu: None,
        })
    }

    fn init_share_dirs(machine: &Machine, id: Id, boot_seq: u64) -> Result<Vec<ShareDir>> {
        let mut share_dirs = vec![];
        for path in machine.config().share_dirs.iter() {
            let share_dir = ShareDir::new(id, boot_seq, &machine, path.clone())
                .context("failed to create share dir")
                .context(id)?;
            share_dirs.push(share_dir);
        }
        Ok(share_dirs)
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn get_mac_address(&self) -> String {
        let id: [u8; 16] = self.id.into();
        let id = &id[id.len() - 3..];
        format!("52:54:00:{:02x}:{:02x}:{:02x}", id[0], id[1], id[2])
    }

    async fn get_qemu_args<Ctx: HasDirs + HasLogger>(&self, ctx: &Ctx) -> Result<Vec<String>> {
        // TODO: could cache if the config has not changed

        let memory = self.machine.config().memory.as_u64().to_string();

        let tap = self.network.get_tap_name(self);
        let mac = self.get_mac_address();
        let net_device = format!("virtio-net-pci,netdev={tap},mac={mac}");
        let netdev = format!("tap,id={tap},ifname={tap},script=no");

        let iso = self.machine.get_cloud_init_iso(ctx).await?;
        let iso = iso.to_string_lossy();

        let iso_drive: String = format!("file={iso},media=cdrom");

        let root_image = self.machine.get_root_image(ctx).await?;
        let root_image = root_image.to_string_lossy();
        let root_drive: String = format!(
            "file={},if=virtio,cache=writeback,discard=ignore,format=qcow2",
            root_image
        );

        let qmp_socket = format!("/tmp/vmm-qmp-{}.sock", self.id.to_string());
        let qmp_socket = format!("unix:{},server,nowait", qmp_socket);

        #[rustfmt::skip]
        let mut args = vec![
            "-machine".into(), "type=pc,accel=kvm".into(),
            "-boot".into(), "d".into(),
            "-smp".into(), self.machine.config().cpus.to_string(),
            "-m".into(), memory.clone() + "B",
            "-device".into(), net_device,
            "-netdev".into(), netdev,
            "-drive".into(), iso_drive,
            "-drive".into(), root_drive,
            "-nographic".into(),
            "-qmp".into(), qmp_socket,
        ];

        for share_dir in self.share_dirs.iter() {
            args.extend(share_dir.get_qemu_args());
        }

        Ok(args)
    }

    pub async fn start<Ctx: HasDirs + HasLogger>(&mut self, ctx: &Ctx) -> Result<()> {
        // TODO: timeout?

        self.network.set_bridge_up_or_create().await?;
        self.network.set_tap_up_or_create(self).await?;

        for share_dir in self.share_dirs.iter_mut() {
            share_dir.start(ctx).await?;
        }

        let qemu_args = self.get_qemu_args(ctx).await?;

        if self.qemu.is_none() {
            self.start_qemu(ctx, qemu_args).await?;
        }

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        // TODO: timeout

        self.stop_qemu().await?;

        for share_dir in self.share_dirs.iter_mut() {
            share_dir.stop().await?;
        }

        Ok(())
    }

    async fn start_qemu<Ctx: HasLogger>(&mut self, ctx: &Ctx, args: Vec<String>) -> Result<()> {
        assert!(self.qemu.is_none(), "qemu is already running");

        let mut child = Command::new("qemu-system-x86_64")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn qemu")
            .context(self.id)?;

        let mut tasks = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let id = self.id.clone();
            let boot_seq = self.boot_seq;
            let mut reader = BufReader::new(stdout).lines();
            let logger = ctx.logger().clone();
            let stdout_task = tokio::spawn(async move {
                while let Ok(Some(line)) = reader.next_line().await {
                    let _ = logger.log(LogLine::instance(
                        id,
                        boot_seq,
                        LogStream::Stdout,
                        LogSource::Virtiofs,
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

        self.qemu = Some((child, tasks));

        Ok(())
    }

    async fn stop_qemu(&mut self) -> Result<()> {
        let Some((mut child, mut tasks)) = self.qemu.take() else {
            return Ok(());
        };

        let status = child
            .wait()
            .await
            .context("failed to wait for qemu")
            .context(self.id)?;

        for task in tasks.drain(..) {
            let _ = task.await;
        }

        if !status.success() {
            anyhow::bail!("qemu exited with {}", status);
        }

        Ok(())
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        assert!(self.qemu.is_none(), "qemu is still running");
    }
}
