use alloy::primitives::Address;

use std::collections::HashMap;

use crate::entities::OclId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OclOwnershipChange {
    pub former_owner: Option<Address>,
    pub current_owner: Address,
}

pub type OclOwnershipDiffMap = HashMap<OclId, OclOwnershipChange>;
