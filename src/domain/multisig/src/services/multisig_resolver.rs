use std::collections::HashSet;

use alloy::primitives::Address;
use async_trait::async_trait;
use color_eyre::eyre;

#[async_trait]
pub trait MultisigResolver {
    async fn get_multisig_owners(
        &self,
        potential_safe_wallet_address: Address,
    ) -> eyre::Result<Option<HashSet<Address>>>;
}
