use alloy::primitives::Address;

// TODO: migrate to figment
//       - use https://github.com/Keats/validator for field validation
#[derive(confique::Config, Debug)]
pub struct Config {
    /// Interface to listen for HTTP admin traffic on
    #[config(env = "KAMU_BRIDGE_HTTP_ADDRESS")]
    #[config(default = "127.0.0.1")]
    pub http_address: std::net::IpAddr,

    /// Port to listen for HTTP admin traffic on
    #[config(env = "KAMU_BRIDGE_HTTP_PORT")]
    #[config(default = 0)]
    pub http_port: u16,

    #[config(env = "KAMU_BRIDGE_KAMU_NODE_GQL_API_ENDPOINT")]
    pub kamu_node_gql_api_endpoint: String,
    #[config(env = "KAMU_BRIDGE_KAMU_NODE_TOKEN")]
    pub kamu_node_token: String,

    #[config(env = "KAMU_BRIDGE_MOLECULE_PROJECTS_DATASET_ALIAS")]
    pub molecule_projects_dataset_alias: String,

    /// ID of the chain that RCP URL is expected to point to
    #[config(env = "KAMU_BRIDGE_CHAIN_ID")]
    #[config(default = 0)]
    pub chain_id: u64,

    #[config(env = "KAMU_BRIDGE_RPC_URL")]
    pub rpc_url: String,

    #[config(env = "KAMU_BRIDGE_IPNFT_CONTRACT_ADDRESS")]
    pub ipnft_contract_address: Address,
    #[config(env = "KAMU_BRIDGE_IPNFT_CONTRACT_BIRTH_BLOCK")]
    pub ipnft_contract_birth_block: u64,

    #[config(env = "KAMU_BRIDGE_TOKENIZER_CONTRACT_ADDRESS")]
    pub tokenizer_contract_address: Address,
    #[config(env = "KAMU_BRIDGE_TOKENIZER_CONTRACT_BIRTH_BLOCK")]
    pub tokenizer_contract_birth_block: u64,
}

impl Config {
    pub fn builder() -> confique::Builder<Config> {
        confique::Config::builder()
    }
}
