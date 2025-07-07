use alloy::primitives::Address;

pub struct Config {
    pub provider_url: String,
    pub ipnft_contract_address: Address,
    pub ipnft_contract_birth_block: u64,
    pub tokenizer_contract_address: Address,
    pub tokenizer_contract_birth_block: u64,
}
