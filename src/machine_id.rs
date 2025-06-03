use anyhow::Result;
use base_62::base62;
use rand_core::{OsRng, TryRngCore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MachineId(u128);

impl MachineId {
    pub fn new() -> Result<Self> {
        let mut bytes = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(MachineId(u128::from_be_bytes(bytes)))
    }
}

impl Into<String> for MachineId {
    fn into(self) -> String {
        base62::encode(&self.0.to_be_bytes())
    }
}

impl Into<String> for &MachineId {
    fn into(self) -> String {
        base62::encode(&self.0.to_be_bytes())
    }
}

impl ToString for MachineId {
    fn to_string(&self) -> String {
        self.into()
    }
}

impl TryFrom<String> for MachineId {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let id = base62::decode(&value).map_err(|e| anyhow::anyhow!(e))?;
        let bytes: [u8; 16] = id
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid base62 string"))?;
        let id = u128::from_be_bytes(bytes);
        Ok(MachineId(id))
    }
}
