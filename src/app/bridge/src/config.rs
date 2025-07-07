use alloy::primitives::Address;

#[derive(confique::Config, Debug)]
pub struct Config {
    #[config(env = "RPC_URL")]
    pub rpc_url: String,

    #[config(env = "IPNFT_CONTRACT_ADDRESS")]
    pub ipnft_contract_address: Address,
    #[config(env = "IPNFT_CONTRACT_BIRTH_BLOCK")]
    pub ipnft_contract_birth_block: u64,

    #[config(env = "TOKENIZER_CONTRACT_ADDRESS")]
    pub tokenizer_contract_address: Address,
    #[config(env = "TOKENIZER_CONTRACT_BIRTH_BLOCK")]
    pub tokenizer_contract_birth_block: u64,
}

impl Config {
    pub fn builder() -> confique::Builder<Config> {
        confique::Config::builder()
    }
}
