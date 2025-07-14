use std::fmt::{Display, Formatter};
use std::str::FromStr;

use alloy::primitives::{Address, U256};
use color_eyre::eyre;
use color_eyre::eyre::{Context, bail};

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone, PartialOrd, Ord)]
pub struct IpnftUid {
    pub ipnft_address: Address,
    pub token_id: U256,
}

impl Display for IpnftUid {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{}", self.ipnft_address, self.token_id)
    }
}

impl FromStr for IpnftUid {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('_').collect();
        if parts.len() != 2 {
            bail!("Invalid format: '{s}'");
        }

        let raw_address = parts[0];
        let raw_token_id = parts[1];

        let ipnft_address = Address::from_str(raw_address)
            .wrap_err_with(|| format!("Address parse error: '{raw_address}'"))?;
        let token_id = U256::from_str(raw_token_id)
            .wrap_err_with(|| format!("Token ID parse error: '{raw_token_id}'"))?;

        Ok(IpnftUid {
            ipnft_address,
            token_id,
        })
    }
}
