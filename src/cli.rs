use anyhow::{Result, bail};
use byte_unit::UnitType;

use crate::{
    args::{Args, Command, MachineCommand, NetworkCommand},
    config::{Machine, Network, parse_config, write_config},
    id::Id,
    supervisor::Supervisor,
    text_table::TextTable,
};

pub fn run_cli(args: Args) -> Result<()> {
    match args.command {
        Command::Machine { command } => match command {
            MachineCommand::List => {
                let mut table = TextTable::build()
                    .add_column("ID")
                    .add_column("Name")
                    .add_column("Network")
                    .add_column("CPUs")
                    .add_column("Memory")
                    .add_column("ISO")
                    .add_column("Boot")
                    .add_column("Virtiofs")
                    .done();

                let config = parse_config(&args.config)?;

                for machine in config.machines {
                    let Some(network) = config.networks.iter().find(|n| n.id == machine.network)
                    else {
                        bail!(
                            "Network with id \"{}\" does not exist",
                            machine.network.to_string()
                        )
                    };
                    table.push(machine.id.to_string());
                    table.push(machine.name);
                    table.push(network.name.clone());
                    table.push(machine.cpus.to_string());
                    table.push(
                        machine
                            .memory
                            .get_appropriate_unit(UnitType::Binary)
                            .to_string(),
                    );
                    table.push(machine.iso.to_string_lossy().into());
                    table.push(machine.boot.to_string_lossy().into());

                    if machine.virtiofs.is_empty() {
                        table.push("".to_string());
                    } else {
                        table.push(
                            machine
                                .virtiofs
                                .iter()
                                .map(|v| v.to_string_lossy().into_owned())
                                .collect::<Vec<String>>()
                                .join(","),
                        );
                    }
                }
                table.print();
            }

            MachineCommand::Create {
                name,
                network,
                cpus,
                memory,
                iso,
                boot,
                virtiofs,
            } => {
                let mut config = parse_config(&args.config)?;
                if !config.networks.iter().any(|n| n.id == network) {
                    anyhow::bail!("Network with id \"{}\" does not exist", network.to_string());
                }
                let machine = Machine {
                    id: Id::new()?,
                    name,
                    network,
                    cpus,
                    memory,
                    iso,
                    boot,
                    virtiofs,
                };
                if config.machines.iter().any(|m| m.name == machine.name) {
                    anyhow::bail!("Machine with name \"{}\" already exists", machine.name);
                }
                config.machines.push(machine);
                write_config(&args.config, config)?;
            }
        },

        Command::Network { command } => match command {
            NetworkCommand::List => {
                let config = parse_config(&args.config)?;
                for network in config.networks {
                    println!("{}", serde_json::to_string_pretty(&network)?);
                }
            }

            NetworkCommand::Create { name, ip } => {
                let mut config = parse_config(&args.config)?;
                let network = Network {
                    id: Id::new()?,
                    name,
                    ip,
                };
                if config.networks.iter().any(|n| n.name == network.name) {
                    anyhow::bail!("Network with name \"{}\" already exists", network.name);
                }
                config.networks.push(network);
                write_config(&args.config, config)?;
            }
        },

        Command::Server => {
            let config = parse_config(&args.config)?;
            let supervisor = Supervisor::new(config)?;
            supervisor.run()?;
        }
    }

    Ok(())
}
