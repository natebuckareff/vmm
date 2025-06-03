use std::{
    collections::{HashMap, HashSet, hash_map},
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use anyhow::{Result, anyhow, bail};
use rand_core::{OsRng, TryRngCore};

use crate::{
    config::{self, Config},
    id::Id,
    net::{
        create_bridge_device, create_tap_device, delete_bridge_device, delete_tap_device,
        get_bridge_name, get_tap_name,
    },
};

pub enum LogStream {
    Stdout,
    Stderr,
}

pub enum LogSource {
    Qemu,
    Virtiofs,
}

pub struct LogLine {
    pub id: Id,
    pub stream: LogStream,
    pub source: LogSource,
    pub line: String,
}

pub struct Machine {
    id: Id,
    network: Id,
    qemu: Child,
    virtiofs: Vec<Child>,
    handles: Vec<JoinHandle<()>>,
}

pub struct Network {
    id: Id,
    children: i32,
}

pub struct Supervisor {
    config: Config,
    machines: HashMap<Id, Machine>,
    networks: HashMap<Id, Network>,
    log_sender: Option<Sender<LogLine>>,
    state_dir: PathBuf,
    machines_dir: PathBuf,
    virtiofs_tags: HashSet<String>,
    log_handle: JoinHandle<()>,
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

        let log_handle = thread::spawn(move || {
            while let Ok(log_line) = log_receiver.recv() {
                let source = match log_line.source {
                    LogSource::Qemu => "qemu",
                    LogSource::Virtiofs => "virtiofs",
                };

                let stream = match log_line.stream {
                    LogStream::Stdout => "stdout",
                    LogStream::Stderr => "stderr",
                };

                println!(
                    "{} {:>8} {} | {}",
                    log_line.id.to_string(),
                    source,
                    stream,
                    log_line.line
                );
            }
        });

        Ok(Self {
            config,
            machines: HashMap::new(),
            networks: HashMap::new(),
            log_sender: Some(log_sender),
            state_dir: state_dir.into(),
            machines_dir,
            virtiofs_tags: HashSet::new(),
            log_handle,
        })
    }

    pub fn run(mut self) -> Result<()> {
        // TODO XXX: this is terrible, need to rethink state management here
        let machines = self.config.machines.clone();
        let networks = self.config.networks.clone();

        for machine in machines {
            let network = networks.iter().find(|n| n.id == machine.network).unwrap();
            self.create_and_start(&machine, &network)?;
        }

        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();

        ctrlc::set_handler(move || {
            eprintln!("SIGINT received, shutting down");
            r.store(false, Ordering::SeqCst);
        })?;

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(1));
        }

        let ids = self.machines.keys().cloned().collect::<Vec<Id>>();
        for id in ids {
            println!("killing and destroying machine {}", id.to_string());
            self.kill_and_destroy(id).unwrap();
        }

        for machine in self.machines.values_mut() {
            println!("joining machine handles for {}", machine.id.to_string());
            for handle in machine.handles.drain(..) {
                handle
                    .join()
                    .map_err(|_| anyhow!("failed to join machine threads"))?;
            }
        }

        self.log_sender.take();

        println!("joining log handle");
        self.log_handle
            .join()
            .map_err(|_| anyhow!("failed to join log thread"))?;

        Ok(())
    }

    fn generate_virtiofs_tag(&mut self) -> Result<String> {
        loop {
            let mut bytes = [0u8; 4];
            OsRng.try_fill_bytes(&mut bytes).map_err(|e| anyhow!(e))?;
            let tag = base_62::encode(&bytes);
            if !self.virtiofs_tags.contains(&tag) {
                self.virtiofs_tags.insert(tag.clone());
                break Ok(tag);
            }
        }
    }

    pub fn create_and_start(
        &mut self,
        machine_config: &config::Machine,
        network_config: &config::Network,
    ) -> Result<bool> {
        let id = machine_config.id;

        let network = match self.networks.entry(machine_config.network) {
            hash_map::Entry::Occupied(entry) => {
                let network = entry.into_mut();
                network.children += 1;
                network
            }
            hash_map::Entry::Vacant(entry) => {
                let bridge = get_bridge_name(&machine_config.network);
                create_bridge_device(&bridge, network_config.ip)?;
                entry.insert(Network {
                    id: machine_config.network,
                    children: 1,
                })
            }
        };

        let tap = get_tap_name(&id);
        create_tap_device(&tap, &get_bridge_name(&network.id))?;

        let qmp_socket = format!("/tmp/vmm-qmp-{}.sock", id.to_string());
        let qmp_socket = format!("unix:{},server,nowait", qmp_socket);

        let memory = machine_config.memory.as_u64().to_string();
        let cpus = machine_config.cpus.to_string();
        let iso = machine_config.iso.to_string_lossy();
        let boot = machine_config.boot.to_string_lossy();
        let mac = format!("52:54:00:{:02x}:{:02x}:{:02x}", 0, 0, 0);
        let net_device = format!("virtio-net-pci,netdev={tap},mac={mac}");
        let netdev = format!("tap,id={tap},ifname={tap},script=no");
        let iso_drive: String = format!("file={iso},media=cdrom");
        let boot_drive: String = format!(
            "file={},if=virtio,cache=writeback,discard=ignore,format=qcow2",
            boot
        );

        #[rustfmt::skip]
        let mut cmdline: Vec<String> = vec![
            "-machine".into(), "type=pc,accel=kvm".into(),
            "-boot".into(), "d".into(),
            "-m".into(), memory.clone() + "B",
            "-smp".into(), cpus,
            "-device".into(), net_device,
            "-netdev".into(), netdev,
            "-drive".into(), iso_drive,
            "-drive".into(), boot_drive,
            "-nographic".into(),
            "-qmp".into(), qmp_socket,
        ];

        let mut virtiofs = vec![];
        let mut handles = vec![];

        for path in machine_config.virtiofs.iter() {
            let tag = self.generate_virtiofs_tag()?;
            let socket = format!("/tmp/vmm-virtiofs-{}-{}.sock", id.to_string(), tag);

            let chardev = format!("socket,id=char-{tag},path={socket}");
            let drive = format!("vhost-user-fs-pci,queue-size=1024,chardev=char-{tag},tag={tag}");
            let shm = "/dev/shm";
            let mem = format!("memory-backend-file,id=mem,size={memory}B,mem-path={shm},share=on");
            let numa = format!("node,memdev=mem");

            cmdline.push("-chardev".into());
            cmdline.push(chardev);

            cmdline.push("-device".into());
            cmdline.push(drive);

            cmdline.push("-object".into());
            cmdline.push(mem);

            cmdline.push("-numa".into());
            cmdline.push(numa);

            let log_sender = self.log_sender.as_ref().unwrap().clone();
            let child = self.spawn_virtiofs_server(id, &tag, &path, log_sender, &mut handles)?;
            virtiofs.push(child);
        }

        dbg!(&cmdline);

        let machine_dir = self.machines_dir.join(id.to_string());
        std::fs::create_dir_all(&machine_dir)?;

        let log_sender = self.log_sender.as_ref().unwrap().clone();
        let qemu = self.spawn_qemu_vm(id, &cmdline, log_sender, &mut handles)?;
        let machine = Machine {
            id,
            network: machine_config.network,
            qemu,
            virtiofs,
            handles,
        };
        self.machines.insert(id, machine);

        Ok(true)
    }

    fn spawn_virtiofs_server(
        &mut self,
        id: Id,
        tag: &str,
        path: &PathBuf,
        log_sender: Sender<LogLine>,
        handles: &mut Vec<JoinHandle<()>>,
    ) -> Result<Child> {
        let socket = format!("/tmp/vmm-virtiofs-{}-{}.sock", id.to_string(), tag);
        let path = path.to_string_lossy();

        #[rustfmt::skip]
        let cmdline = vec![
            "--socket-path", &socket,
            "--shared-dir", &path,
            "--tag", tag,
        ];

        // TODO: why is this not on path?
        let mut child = Command::new("/usr/lib/virtiofsd")
            .args(cmdline)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(out) = child.stdout.take() {
            let sender = log_sender.clone();
            let handle = thread::spawn(move || {
                for line in BufReader::new(out).lines().flatten() {
                    let _ = sender.send(LogLine {
                        id,
                        stream: LogStream::Stdout,
                        source: LogSource::Virtiofs,
                        line,
                    });
                }
            });
            handles.push(handle);
        }

        if let Some(err) = child.stderr.take() {
            let sender = log_sender.clone();
            let handle = thread::spawn(move || {
                for line in BufReader::new(err).lines().flatten() {
                    let _ = sender.send(LogLine {
                        id,
                        stream: LogStream::Stderr,
                        source: LogSource::Virtiofs,
                        line,
                    });
                }
            });
            handles.push(handle);
        }

        Ok(child)
    }

    fn spawn_qemu_vm(
        &mut self,
        id: Id,
        cmdline: &[String],
        log_sender: Sender<LogLine>,
        handles: &mut Vec<JoinHandle<()>>,
    ) -> Result<Child> {
        let mut child = Command::new("qemu-system-x86_64")
            .args(cmdline)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(out) = child.stdout.take() {
            let sender = log_sender.clone();
            let handle = thread::spawn(move || {
                for line in BufReader::new(out).lines().flatten() {
                    let _ = sender.send(LogLine {
                        id,
                        stream: LogStream::Stdout,
                        source: LogSource::Qemu,
                        line,
                    });
                }
            });
            handles.push(handle);
        }

        if let Some(err) = child.stderr.take() {
            let sender = log_sender.clone();
            let handle = thread::spawn(move || {
                for line in BufReader::new(err).lines().flatten() {
                    let _ = sender.send(LogLine {
                        id,
                        stream: LogStream::Stderr,
                        source: LogSource::Qemu,
                        line,
                    });
                }
            });
            handles.push(handle);
        }

        Ok(child)
    }

    pub fn kill_and_destroy(&mut self, id: Id) -> Result<()> {
        dbg!("kill_and_destroy", &id);

        let mut machine = self
            .machines
            .remove(&id)
            .ok_or(anyhow::anyhow!("machine not found"))?;

        dbg!("kill_and_destroy", "qemu.kill()", &id);
        machine.qemu.kill()?;

        for virtiofs in machine.virtiofs.iter_mut() {
            dbg!("kill_and_destroy", "virtiofs.kill()", &id);
            virtiofs.kill()?;
        }

        let tap = get_tap_name(&id);
        delete_tap_device(&tap)?;

        let Some(network) = self.networks.get_mut(&machine.network) else {
            bail!("network not found");
        };

        network.children -= 1;

        if network.children == 0 {
            let bridge = get_bridge_name(&machine.network);
            delete_bridge_device(&bridge)?;
        }

        Ok(())
    }
}
