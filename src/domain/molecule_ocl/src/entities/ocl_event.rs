use alloy::primitives::Address;
use molecule_contracts::LabNFT;

use crate::entities::OclId;

// NOTE: reduced type (w/o tokenId)
#[derive(Debug)]
pub struct OclTransferEvent {
    pub ocl_id: OclId,
    pub from: Address,
    pub to: Address,
}

impl From<LabNFT::OclTransfer> for OclTransferEvent {
    fn from(value: LabNFT::OclTransfer) -> Self {
        OclTransferEvent {
            ocl_id: value.oclId.into(),
            from: value.from,
            to: value.to,
        }
    }
}
