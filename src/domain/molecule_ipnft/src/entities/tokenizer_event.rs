use alloy::primitives::{Address, U256};

#[derive(Debug)]
pub enum TokenizerEvent {
    TokenCreated(TokenizerEventTokenCreated),
}

#[derive(Debug)]
pub struct TokenizerEventTokenCreated {
    pub symbol: String,
    pub token_id: U256,
    pub token_address: Address,
    pub birth_block: u64,
}
