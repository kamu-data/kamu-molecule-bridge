use std::fmt::{Display, Formatter};

use alloy::primitives::Address;
use color_eyre::eyre;
use color_eyre::eyre::bail;

#[derive(Debug, Copy, Clone)]
pub struct DidPhk {
    caip2: &'static str,
    address: Address,
}

impl DidPhk {
    pub fn new_from_chain_id(chain_id: u64, address: Address) -> eyre::Result<Self> {
        let caip2 = Self::get_caip2(chain_id)?;
        Ok(Self { caip2, address })
    }

    fn get_caip2(chain_id: u64) -> eyre::Result<&'static str> {
        match chain_id {
            1 => Ok("eip155:1"),
            11155111 => Ok("eip155:11155111"),

            _ => bail!("Unsupported network with chain ID: {chain_id}"),
        }
    }
}

impl Display for DidPhk {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "did:pkh:{}:{}", self.caip2, self.address)
    }
}
