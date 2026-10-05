use alloy::primitives::Address;

#[derive(confique::Config, Debug)]
pub struct Config {
    /// Interface to listen for HTTP admin traffic on
    #[config(env = "KAMU_MOLECULE_BRIDGE_HTTP_ADDRESS")]
    #[config(default = "127.0.0.1")]
    pub http_address: std::net::IpAddr,

    /// Port to listen for HTTP admin traffic on
    #[config(env = "KAMU_MOLECULE_BRIDGE_HTTP_PORT")]
    #[config(default = 0)]
    pub http_port: u16,

    #[config(env = "KAMU_MOLECULE_BRIDGE_KAMU_NODE_GQL_API_ENDPOINT")]
    pub kamu_node_gql_api_endpoint: String,
    #[config(env = "KAMU_MOLECULE_BRIDGE_KAMU_NODE_TOKEN")]
    pub kamu_node_token: String,

    #[config(env = "KAMU_MOLECULE_BRIDGE_MOLECULE_PROJECTS_DATASET_ALIAS")]
    pub molecule_projects_dataset_alias: String,

    /// Minimal period of time before fetching projects and data room updates from kamu node
    #[config(env = "KAMU_MOLECULE_BRIDGE_MOLECULE_PROJECTS_LOADING_INTERVAL_IN_SECS")]
    pub molecule_projects_loading_interval_in_secs: u64,

    /// ID of the chain that RCP URL is expected to point to
    #[config(env = "KAMU_MOLECULE_BRIDGE_CHAIN_ID")]
    pub chain_id: u64,

    #[config(env = "KAMU_MOLECULE_BRIDGE_RPC_URL")]
    pub rpc_url: String,

    #[config(env = "KAMU_MOLECULE_BRIDGE_LABNFT_CONTRACT_ADDRESS")]
    pub labnft_contract_address: Address,
    #[config(env = "KAMU_MOLECULE_BRIDGE_LABNFT_CONTRACT_BIRTH_BLOCK")]
    pub labnft_contract_birth_block: u64,

    /// Period between the main update loop runs (includes chain indexing and all other syncs)
    #[config(env = "KAMU_MOLECULE_BRIDGE_INDEXING_DELAY_BETWEEN_ITERATIONS_IN_SECS")]
    pub indexing_delay_between_iterations_in_secs: u64,

    /// Number of data rooms queried per SQL batch
    #[config(env = "KAMU_MOLECULE_BRIDGE_DATA_ROOM_BATCH_SIZE")]
    #[config(default = 128)]
    pub data_room_batch_size: usize,

    /// Maximum number of concurrent data room batch queries
    #[config(env = "KAMU_MOLECULE_BRIDGE_MAX_CONCURRENT_DATA_ROOM_BATCHES")]
    #[config(default = 4)]
    pub max_concurrent_data_room_batches: usize,

    /// List of OCL ids that should be ignored
    #[config(env = "KAMU_MOLECULE_BRIDGE_IGNORE_OCL_IDS", parse_env = confique::env::parse::list_by_comma)]
    pub ignore_ocl_ids: Option<std::collections::HashSet<String>>,
}

impl Config {
    pub fn builder() -> confique::Builder<Config> {
        confique::Config::builder()
    }
}
