use alloy::primitives::{Address, U256};

#[derive(Debug)]
pub enum IptEvent {
    Transfer(IptEventTransfer),
}

#[derive(Debug)]
pub struct IptEventTransfer {
    pub token_address: Address,
    pub from: Address,
    pub to: Address,
    pub value: U256,
}
