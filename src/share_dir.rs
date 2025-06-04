use std::{cell::OnceCell, path::PathBuf, process::Stdio};

use anyhow::{Context, Result, anyhow};
use byte_unit::Byte;
use rand_core::{OsRng, TryRngCore};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
};

use crate::{
    ctx::HasLogger,
    id::Id,
    logger::{LogLine, LogSource, LogStream},
    machine::Machine,
};

pub struct ShareDir {
    instance_id: Id,
    instance_memory: Byte,
    tag: String,
    path: PathBuf,
    socket_path: OnceCell<PathBuf>,
    daemon: Option<(Child, Vec<JoinHandle<()>>)>,
}

impl ShareDir {
    pub fn new(instance_id: Id, machine: &Machine, path: PathBuf) -> Result<Self> {
        let instance_memory = machine.config().memory;
        loop {
            let mut bytes = [0u8; 4];
            OsRng.try_fill_bytes(&mut bytes).map_err(|e| anyhow!(e))?;
            let tag = base_62::encode(&bytes);
            let sharer_dir = Self {
                instance_id,
                instance_memory,
                tag,
                path: path.clone(),
                socket_path: OnceCell::new(),
                daemon: None,
            };
            if !sharer_dir.path.exists() {
                break Ok(sharer_dir);
            }
        }
    }

    pub fn get_socket_path(&self) -> &PathBuf {
        self.socket_path.get_or_init(|| {
            let socket = format!(
                "/tmp/vmm-virtiofs-{}-{}.sock",
                self.instance_id.to_string(),
                self.tag
            );
            PathBuf::from(socket)
        })
    }

    pub fn get_qemu_args(&self) -> Vec<String> {
        let chardev = format!(
            "socket,id=char-{},path={}",
            self.tag,
            self.get_socket_path().to_string_lossy()
        );

        let device = format!(
            "vhost-user-fs-pci,queue-size=1024,chardev=char-{tag},tag={tag}",
            tag = self.tag
        );

        let memory = self.instance_memory.as_u64().to_string();
        let shm = "/dev/shm";

        let mem = format!("memory-backend-file,id=mem,size={memory}B,mem-path={shm},share=on");
        let numa = format!("node,memdev=mem");

        #[rustfmt::skip]
        let args = vec![
            "-chardev".into(), chardev,
            "-device".into(), device,
            "-object".into(), mem,
            "-numa".into(), numa,
        ];

        args
    }

    async fn start_virtiofsd<Ctx: HasLogger>(&mut self, ctx: &Ctx) -> Result<()> {
        assert!(self.daemon.is_none(), "virtiofsd already running");

        let socket_path = self.get_socket_path().to_string_lossy();
        let path = self.path.to_string_lossy();

        #[rustfmt::skip]
        let args = vec![
            "--socket-path", &socket_path,
            "--shared-dir", &path,
            "--tag", &self.tag,
        ];

        let mut child = Command::new("/usr/lib/virtiofsd")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn virtiofsd")
            .context(self.instance_id)?;

        let mut tasks = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let id = self.instance_id.clone();
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
            let id = self.instance_id.clone();
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

        self.daemon = Some((child, tasks));

        Ok(())
    }

    pub async fn start<Ctx: HasLogger>(&mut self, ctx: &Ctx) -> Result<bool> {
        if self.daemon.is_some() {
            return Ok(false);
        }
        self.start_virtiofsd(ctx).await?;
        Ok(true)
    }

    pub async fn stop(&mut self) -> Result<bool> {
        let Some((mut child, mut tasks)) = self.daemon.take() else {
            return Ok(false);
        };

        let status = child
            .wait()
            .await
            .context("failed to wait for virtiofsd")
            .context(self.instance_id)?;

        for task in tasks.drain(..) {
            let _ = task.await;
        }

        if !status.success() {
            anyhow::bail!("virtiofsd exited with {}", status);
        }

        Ok(true)
    }
}

impl Drop for ShareDir {
    fn drop(&mut self) {
        assert!(self.daemon.is_none(), "virtiofsd is still running");
    }
}
