use std::process::Stdio;

use anyhow::{Context, Result};
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

pub struct Instance {
    id: Id,
    machine: Machine,
    network: Network,
    share_dirs: Vec<ShareDir>,
    qemu: Option<(Child, Vec<JoinHandle<()>>)>,
}

impl Instance {
    pub fn new(id: Id, machine: Machine, network: Network) -> Result<Self> {
        let mut share_dirs = vec![];
        for path in machine.config().share_dirs.iter() {
            let share_dir = ShareDir::new(id, &machine, path.clone())
                .context("failed to create share dir")
                .context(id)?;
            share_dirs.push(share_dir);
        }

        Ok(Self {
            id,
            machine,
            network,
            share_dirs,
            qemu: None,
        })
    }

    pub fn id(&self) -> &Id {
        &self.id
    }

    pub fn machine(&self) -> &Machine {
        &self.machine
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

    pub async fn stop<Ctx: HasLogger>(&mut self) -> Result<()> {
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
            let mut reader = BufReader::new(stdout).lines();
            let logger = ctx.logger().clone();
            let stdout_task = tokio::spawn(async move {
                while let Ok(Some(line)) = reader.next_line().await {
                    let _ = logger.log(LogLine::instance(
                        id,
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
