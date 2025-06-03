use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
pub struct Cli {
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
}

#[derive(Debug, Subcommand)]
pub enum MachineCommand {
    List,
    Create { name: String },
}
