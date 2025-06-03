use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Sender},
    thread,
};

use anyhow::Result;

use crate::{config::Config, machine_id::MachineId};

pub enum Stream {
    Stdout,
    Stderr,
}

pub struct LogLine {
    pub id: MachineId,
    pub stream: Stream,
    pub line: String,
}

pub struct Machine {
    id: MachineId,
    child: Child,
}

struct Supervisor {
    config: Config,
    machines: HashMap<MachineId, Machine>,
    log_sender: Sender<LogLine>,
    state_dir: PathBuf,
    machines_dir: PathBuf,
}

impl Supervisor {
    pub fn new(config: Config) -> Result<Self> {
        let base_dirs = directories::BaseDirs::new().ok_or(anyhow::anyhow!("no base dirs"))?;
        let state_dir = base_dirs
            .state_dir()
            .ok_or(anyhow::anyhow!("no state dir"))?;

        let machines_dir = state_dir.join("machines");
        std::fs::create_dir_all(&machines_dir)?;

        let (log_sender, log_receiver) = mpsc::channel::<LogLine>();

        thread::spawn(move || {
            while let Ok(log_line) = log_receiver.recv() {
                let source = match log_line.stream {
                    Stream::Stdout => "stdout",
                    Stream::Stderr => "stderr",
                };
                println!("{} {}: {}", log_line.id.to_string(), source, log_line.line);
            }
        });

        Ok(Self {
            config,
            machines: HashMap::new(),
            log_sender,
            state_dir: state_dir.into(),
            machines_dir,
        })
    }

    fn start(&mut self, id: MachineId) -> Result<()> {
        let qmp_socket = format!("/tmp/vmm-qmp-{}", id.to_string());
        let qmp_socket = format!("unix:{},server,nowait", qmp_socket);

        #[rustfmt::skip]
        let cmdline = vec![
            "-m", "1024",
            "-smp", "2",
            "-nographic",
            "-qmp", &qmp_socket,
        ];

        let machine_dir = self.machines_dir.join(id.to_string());
        std::fs::create_dir_all(&machine_dir)?;

        let child = self.spawn_qemu_vm(id, &cmdline, self.log_sender.clone())?;
        let machine = Machine { id, child };
        self.machines.insert(id, machine);

        Ok(())
    }

    fn spawn_qemu_vm(
        &self,
        id: MachineId,
        cmdline: &[&str],
        log_sender: Sender<LogLine>,
    ) -> anyhow::Result<Child> {
        let mut child = Command::new("qemu-system-x86_64")
            .args(cmdline)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(out) = child.stdout.take() {
            let sender = log_sender.clone();
            thread::spawn(move || {
                for line in BufReader::new(out).lines().flatten() {
                    let _ = sender.send(LogLine {
                        id,
                        stream: Stream::Stdout,
                        line,
                    });
                }
            });
        }

        if let Some(err) = child.stderr.take() {
            let sender = log_sender.clone();
            thread::spawn(move || {
                for line in BufReader::new(err).lines().flatten() {
                    let _ = sender.send(LogLine {
                        id,
                        stream: Stream::Stderr,
                        line,
                    });
                }
            });
        }

        Ok(child)
    }
}
