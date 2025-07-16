use alloy::primitives::Address;

use crate::entities::{TokenizerEvent, TokenizerEventTokenCreated};

pub struct TokenizerEventProcessingResponse {
    pub new_ipt_addresses: Vec<Address>,
    pub minimal_ipt_birth_block: u64,
}

pub struct TokenizerEventProcessingStrategy;

impl TokenizerEventProcessingStrategy {
    pub fn process(events: Vec<TokenizerEvent>) -> TokenizerEventProcessingResponse {
        let mut new_ipt_addresses = Vec::with_capacity(events.len());
        let mut minimal_ipt_birth_block = 0;

        for event in events {
            match event {
                TokenizerEvent::TokenCreated(TokenizerEventTokenCreated {
                    token_contract,
                    symbol: _,
                    block_number,
                }) => {
                    new_ipt_addresses.push(token_contract);

                    minimal_ipt_birth_block = minimal_ipt_birth_block.min(block_number);
                }
            }
        }

        TokenizerEventProcessingResponse {
            new_ipt_addresses,
            minimal_ipt_birth_block,
        }
    }
}
