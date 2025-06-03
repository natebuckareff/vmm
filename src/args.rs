use std::path::PathBuf;

use byte_unit::Byte;
use clap::{Parser, Subcommand};
use ipnet::Ipv4Net;

use crate::id::Id;

#[derive(Debug, Parser)]
pub struct Args {
    #[clap(short, long)]
    pub config: PathBuf,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Machine {
        #[clap(subcommand)]
        command: MachineCommand,
    },

    Network {
        #[clap(subcommand)]
        command: NetworkCommand,
    },

    Server,
}

#[derive(Debug, Subcommand)]
pub enum MachineCommand {
    List,
    Create {
        #[clap(short('n'), long)]
        name: String,

        #[clap(short('N'), long)]
        network: Id,

        #[clap(short, long)]
        cpus: u8,

        #[clap(short, long)]
        memory: Byte,

        #[clap(short, long)]
        iso: PathBuf,

        #[clap(short, long)]
        boot: PathBuf,

        #[clap(short, long)]
        virtiofs: Vec<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum NetworkCommand {
    List,
    Create { name: String, ip: Ipv4Net },
}
