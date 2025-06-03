use anyhow::{Result, bail};
use byte_unit::UnitType;
use clap::Parser;

use crate::{
    args::{Args, Command, MachineCommand, NetworkCommand},
    id::Id,
    text_table::TextTable,
};

mod args;
mod cli;
mod config;
mod id;
mod net;
mod supervisor;
mod text_table;

fn main() -> Result<()> {
    let args = Args::parse();
    cli::run_cli(args)
}
