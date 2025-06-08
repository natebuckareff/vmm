use anyhow::{Result, bail};
use byte_unit::UnitType;
use clap::Parser;

use crate::{
    args::{Args, Command, MachineCommand, NetworkCommand},
    ctx::Ctx,
    machine::MachineConfig,
    network::NetworkConfig,
    text_table::TextTable,
};

pub struct Cli {
    ctx: Ctx,
}

impl Cli {
    pub fn new() -> Self {
        Self { ctx: Ctx::new() }
    }

    pub async fn run(self) -> Result<()> {
        let args = Args::parse();

        match args.command {
            Command::Machine { command } => match command {
                MachineCommand::List => {
                    let mut table = TextTable::build()
                        .add_column("ID")
                        .add_column("Name")
                        .add_column("CPUs")
                        .add_column("Memory")
                        .add_column("Image")
                        .add_column("Share Dirs")
                        .add_column("Network")
                        .done();

                    let machine_ids = self.ctx.dirs().get_machine_config_ids()?;
                    let network_ids = self.ctx.dirs().get_network_config_ids()?;

                    for machine_id in machine_ids {
                        let machine = MachineConfig::open(&self.ctx, machine_id).await?;

                        let Some(network_id) =
                            network_ids.iter().find(|id| **id == machine.network.id)
                        else {
                            bail!(
                                "Network with id \"{}\" does not exist",
                                machine.network.id.to_string()
                            )
                        };

                        let network = NetworkConfig::open(&self.ctx, *network_id).await?;

                        table.push(machine_id.to_string());
                        table.push(machine.name);
                        table.push(machine.cpus.to_string());
                        table.push(
                            machine
                                .memory
                                .get_appropriate_unit(UnitType::Binary)
                                .to_string(),
                        );
                        table.push(machine.image.url.to_string());

                        if machine.share_dirs.is_empty() {
                            table.push("".to_string());
                        } else {
                            table.push(
                                machine
                                    .share_dirs
                                    .iter()
                                    .map(|v| v.to_string_lossy().into_owned())
                                    .collect::<Vec<String>>()
                                    .join(","),
                            );
                        }

                        table.push(network.name.clone());
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
                    todo!()
                }
            },

            Command::Network { command } => match command {
                NetworkCommand::List => {
                    todo!()
                }

                NetworkCommand::Create { name, ip } => {
                    todo!()
                }
            },

            Command::Server => {
                todo!()
            }
        }

        Ok(())
    }
}
