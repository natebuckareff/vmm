use std::str::FromStr;

use anyhow::Result;
use base_62::base62;
use rand_core::{OsRng, TryRngCore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(u128);

impl Id {
    pub fn new() -> Result<Self> {
        let mut bytes = [0u8; 16];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Id(u128::from_be_bytes(bytes)))
    }
}

impl Into<String> for Id {
    fn into(self) -> String {
        base62::encode(&self.0.to_be_bytes())
    }
}

impl Into<String> for &Id {
    fn into(self) -> String {
        base62::encode(&self.0.to_be_bytes())
    }
}

impl Into<[u8; 16]> for Id {
    fn into(self) -> [u8; 16] {
        self.0.to_be_bytes()
    }
}

impl Into<[u8; 16]> for &Id {
    fn into(self) -> [u8; 16] {
        self.0.to_be_bytes()
    }
}

impl ToString for Id {
    fn to_string(&self) -> String {
        self.into()
    }
}

impl FromStr for Id {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let id = base62::decode(s).map_err(|e| anyhow::anyhow!(e))?;
        let bytes: [u8; 16] = id
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid base62 string"))?;
        let id = u128::from_be_bytes(bytes);
        Ok(Id(id))
    }
}

impl TryFrom<String> for Id {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_str(&value)
    }
}
