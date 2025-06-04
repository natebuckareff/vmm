use anyhow::Result;
use clap::Parser;

use crate::args::Args;

mod args;
mod cli;
mod config;
mod ctx;
mod id;
mod instance;
mod logger;
mod machine;
mod net;
mod network;
mod share_dir;
mod supervisor;
mod text_table;
mod vmm_dirs;

fn main() -> Result<()> {
    let args = Args::parse();
    cli::run_cli(args)
}
