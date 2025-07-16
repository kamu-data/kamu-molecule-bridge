use alloy::primitives::Address;
use serde::Serialize;

use crate::entities::ipnft_uid::IpnftUid;

#[derive(Debug)]
pub enum IpnftEvent {
    Minted(IpnftEventMinted),
    Transfer(IpnftEventTransfer),
    Burnt(IpnftEventBurnt),
}

impl IpnftEvent {
    pub fn ipnft_uid(&self) -> IpnftUid {
        match self {
            IpnftEvent::Minted(minted) => minted.ipnft_uid,
            IpnftEvent::Transfer(transfer) => transfer.ipnft_uid,
            IpnftEvent::Burnt(burnt) => burnt.ipnft_uid,
        }
    }
}

#[derive(Debug)]
pub struct IpnftEventMinted {
    pub ipnft_uid: IpnftUid,
    pub initial_owner: Address,
    pub symbol: String,
}

#[derive(Debug)]
pub struct IpnftEventTransfer {
    pub ipnft_uid: IpnftUid,
    pub from: Address,
    pub to: Address,
}

#[derive(Debug)]
pub struct IpnftEventBurnt {
    pub ipnft_uid: IpnftUid,
    pub former_owner: Address,
}

#[derive(Debug, Default, Serialize)]
pub struct IpnftEventProjection {
    pub symbol: Option<String>,
    pub current_owner: Option<Address>,
    pub former_owner: Option<Address>,
    pub minted: bool,
    pub burnt: bool,
}
