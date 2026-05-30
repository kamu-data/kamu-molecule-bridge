use std::collections::HashSet;

use alloy::primitives::Address;

#[cfg_attr(any(feature = "testing", test), mockall::automock)]
#[async_trait::async_trait]
pub trait MultisigResolver {
    async fn get_multisig_owners(&self, address: Address)
    -> eyre::Result<Option<HashSet<Address>>>;
}
