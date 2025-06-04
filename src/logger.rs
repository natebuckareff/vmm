use std::{
    fs::{self, OpenOptions},
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;

use crate::{id::Id, vmm_dirs::VmmDirs};

pub struct LogLine {
    id: LogId,
    when: SystemTime,
    stream: LogStream,
    source: LogSource,
    line: String,
}

impl LogLine {
    pub fn machine(id: Id, stream: LogStream, source: LogSource, line: String) -> Self {
        Self {
            id: LogId::Machine(id),
            when: SystemTime::now(),
            stream,
            source,
            line,
        }
    }

    pub fn instance(
        id: Id,
        boot_seq: u64,
        stream: LogStream,
        source: LogSource,
        line: String,
    ) -> Self {
        Self {
            id: LogId::Instance(id, boot_seq),
            when: SystemTime::now(),
            stream,
            source,
            line,
        }
    }
}

pub enum LogId {
    Machine(Id),
    Instance(Id, u64),
}

pub enum LogStream {
    Stdout,
    Stderr,
}

impl AsRef<str> for LogStream {
    fn as_ref(&self) -> &str {
        match self {
            LogStream::Stdout => "stdout",
            LogStream::Stderr => "stderr",
        }
    }
}

pub enum LogSource {
    CloudInit,
    Qemu,
    Virtiofs,
}

impl AsRef<str> for LogSource {
    fn as_ref(&self) -> &str {
        match self {
            LogSource::CloudInit => "cloud-init",
            LogSource::Qemu => "qemu",
            LogSource::Virtiofs => "virtiofs",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Logger {
    dirs: VmmDirs,
}

impl Logger {
    pub fn new(dirs: VmmDirs) -> Self {
        Self { dirs }
    }

    pub fn log(&self, log: LogLine) -> Result<()> {
        // TODO: can speed this up by caching log files

        let (path, seq) = match log.id {
            LogId::Machine(id) => {
                let path = self.dirs.get_machine_log_dir(id)?;
                fs::create_dir_all(&path)?;
                (path, None)
            }
            LogId::Instance(id, seq) => {
                let path = self.dirs.get_instance_log_dir(id)?;
                fs::create_dir_all(&path)?;
                (path, Some(seq))
            }
        };

        let days_since_epoch = log.when.duration_since(UNIX_EPOCH)?.as_secs() / 86_400;

        let mut file = match seq {
            Some(boot_seq) => OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.join(format!(
                    "{}.{}-{}.{}",
                    log.source.as_ref(),
                    days_since_epoch,
                    boot_seq,
                    log.stream.as_ref(),
                )))?,

            None => OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.join(format!(
                    "{}.{}.{}",
                    log.source.as_ref(),
                    days_since_epoch,
                    log.stream.as_ref(),
                )))?,
        };

        file.write_all(log.line.as_bytes())?;

        Ok(())
    }
}
