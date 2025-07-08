use std::fmt::{Display, Formatter};

use alloy::primitives::{Address, U256};

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
