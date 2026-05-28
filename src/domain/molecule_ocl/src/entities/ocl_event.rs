use alloy::primitives::Address;

use crate::entities::OclId;

#[derive(Debug)]
pub struct OclTransferEvent {
    pub ocl_id: OclId,
    pub from: Address,
    pub to: Address,
}
