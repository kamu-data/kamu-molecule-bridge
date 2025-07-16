use alloy::primitives::Address;

// TODO: migrate to figment
//       - use https://github.com/Keats/validator for field validation
#[derive(confique::Config, Debug)]
pub struct Config {
    #[config(env = "KAMU_NODE_GQL_API_ENDPOINT")]
    pub kamu_node_gql_api_endpoint: String,
    #[config(env = "KAMU_NODE_TOKEN")]
    pub kamu_node_token: String,

    #[config(env = "MOLECULE_PROJECTS_DATASET_ALIAS")]
    pub molecule_projects_dataset_alias: String,

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

    #[config(env = "INDEXING_DELAY_BETWEEN_ITERATIONS_IN_SECS")]
    pub indexing_delay_between_iterations_in_secs: u64,
}

impl Config {
    pub fn builder() -> confique::Builder<Config> {
        confique::Config::builder()
    }
}
