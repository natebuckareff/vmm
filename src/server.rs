use std::collections::HashMap;

use anyhow::{Context, Result, anyhow, bail};

use crate::{
    ctx::Ctx,
    id::Id,
    instance::Instance,
    machine::{Machine, MachineConfig},
    network::{Network, NetworkConfig},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EntityKind {
    Machine,
    Network,
}

pub struct Server {
    names: HashMap<(EntityKind, String), Id>,
    machines: HashMap<Id, Machine>,
    networks: HashMap<Id, Network>,
    instances: HashMap<Id, Instance>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            names: HashMap::new(),
            machines: HashMap::new(),
            networks: HashMap::new(),
            instances: HashMap::new(),
        }
    }

    async fn read_machines(&mut self, ctx: &Ctx) -> Result<()> {
        let config = ctx.dirs();
        let ids = config.get_machine_config_ids()?;
        for id in ids {
            let machine = Machine::open(ctx, id).await?;
            let name = machine.config().name.clone();
            if self.names.insert((EntityKind::Machine, name), id).is_some() {
                bail!("machine name already exists: {}", machine.config().name);
            }
            self.machines.insert(id, machine);
        }
        Ok(())
    }

    async fn read_networks(&mut self, ctx: &Ctx) -> Result<()> {
        let config = ctx.dirs();
        let ids = config.get_network_config_ids()?;
        for id in ids {
            let network = Network::open(ctx, id).await?;
            let name = network.config().name.clone();
            if self.names.insert((EntityKind::Network, name), id).is_some() {
                bail!("network name already exists: {}", network.config().name);
            }
            self.networks.insert(id, network);
        }
        Ok(())
    }

    // XXX TODO: do we even use config for instances?
    async fn read_instances(&mut self, ctx: &Ctx) -> Result<()> {
        let state = ctx.dirs();
        let ids = state.get_instance_state_ids()?;
        for id in ids {
            let instance = Instance::read(ctx, id).await?;
            self.instances.insert(id, instance);
        }
        Ok(())
    }

    pub async fn read_all(&mut self, ctx: &Ctx) -> Result<()> {
        self.read_machines(ctx).await?;
        self.read_networks(ctx).await?;
        self.read_instances(ctx).await?;
        Ok(())
    }

    pub async fn create_machine(&mut self, ctx: &Ctx, config: MachineConfig) -> Result<Id> {
        let id = loop {
            let id = Id::new()?;
            if !self.machines.contains_key(&id) {
                break id;
            }
        };
        let machine = Machine::new(ctx, id, config).await?;
        self.machines.insert(id, machine);
        Ok(id)
    }

    pub async fn create_network(&mut self, ctx: &Ctx, config: NetworkConfig) -> Result<Id> {
        let id = loop {
            let id = Id::new()?;
            if !self.networks.contains_key(&id) {
                break id;
            }
        };
        let network = Network::new(ctx, id, config).await?;
        self.networks.insert(id, network);
        Ok(id)
    }

    pub async fn create_instance(
        &mut self,
        ctx: &Ctx,
        machine_id: Id,
        network_id: Id,
    ) -> Result<Id> {
        let id = loop {
            let id = Id::new()?;
            if !self.instances.contains_key(&id) {
                break id;
            }
        };

        let machine = self
            .machines
            .get(&machine_id)
            .ok_or(anyhow::anyhow!("machine not found"))?;

        let network = self
            .networks
            .get(&network_id)
            .ok_or(anyhow::anyhow!("network not found"))?;

        let instance = Instance::new(ctx, id, machine.clone(), network.clone()).await?;
        self.instances.insert(id, instance);

        Ok(id)
    }

    pub async fn start_instance(&mut self, ctx: &Ctx, id: &Id) -> Result<()> {
        let instance = self
            .instances
            .get_mut(&id)
            .ok_or(anyhow!("instance not found"))?;

        instance
            .start(ctx)
            .await
            .context("failed to start instance")
            .context(*id)?;

        Ok(())
    }

    pub async fn stop_instance(&mut self, ctx: &Ctx, id: Id) -> Result<()> {
        let instance = self
            .instances
            .get_mut(&id)
            .ok_or(anyhow!("instance not found"))?;

        instance
            .stop()
            .await
            .context("failed to stop instance")
            .context(id)?;

        Ok(())
    }
}
