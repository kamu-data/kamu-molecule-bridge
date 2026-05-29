use alloy::primitives::Address;

use std::collections::HashMap;

use crate::entities::OclId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OclOwnershipChange {
    pub former_owner: Option<Address>,
    // Note: no burn function, so no need for Option<Address>
    // https://github.com/moleculeprotocol/onchainlabs/blob/c69b3774a887906a3a05983d4a410847a189a779/docs/nft/labnft-solady-migration-plan.md?plain=1#L704
    pub current_owner: Address,
}

pub type OclOwnershipDiffMap = HashMap<OclId, OclOwnershipChange>;
