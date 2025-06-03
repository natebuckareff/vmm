use anyhow::Result;
use clap::Parser;

use crate::{
    cli::{Cli, Command, MachineCommand},
    machine_id::MachineId,
};

mod bridge;
mod cli;
mod config;
mod machine_id;
mod supervisor;

fn main() -> Result<()> {
    let args = Cli::parse();

    match args.command {
        Command::Machine { command } => match command {
            MachineCommand::List => {
                let config = config::parse_config(&args.config)?;
                for machine in config.machines {
                    println!("{}", serde_json::to_string_pretty(&machine)?);
                }
            }

            MachineCommand::Create { name } => {
                let mut config = config::parse_config(&args.config)?;
                let machine = config::Machine {
                    id: MachineId::new()?,
                    name,
                };
                if config.machines.iter().any(|m| m.name == machine.name) {
                    anyhow::bail!("Machine with name \"{}\" already exists", machine.name);
                }
                config.machines.push(machine);
                config::write_config(&args.config, config)?;
            }
        },
    }

    Ok(())
}
