use alloy::primitives::Address;

#[derive(Debug)]
pub enum TokenizerEvent {
    TokenCreated(TokenizerEventTokenCreated),
}

#[derive(Debug)]
pub struct TokenizerEventTokenCreated {
    pub token_contract: Address,
    pub symbol: String,
    pub block_number: u64,
}
