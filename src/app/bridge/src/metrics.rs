pub struct BridgeMetrics {
    pub evm_rpc_requests_num_total: prometheus::IntCounter,
    pub evm_rpc_errors_num_total: prometheus::IntCounter,
    pub kamu_gql_requests_num_total: prometheus::IntCounter,
    pub kamu_gql_errors_num_total: prometheus::IntCounter,
}

impl BridgeMetrics {
    pub fn new(chain_id: u64) -> Self {
        use prometheus::*;

        Self {
            evm_rpc_requests_num_total: IntCounter::with_opts(
                Opts::new(
                    "evm_rpc_requests_num_total",
                    "Number of EVM node RPC requests executed",
                )
                .const_label("chain_id", chain_id.to_string()),
            )
            .unwrap(),
            evm_rpc_errors_num_total: IntCounter::with_opts(
                Opts::new(
                    "evm_rpc_errors_num_total",
                    "Number of EVM node RPC requests that resulted in an error",
                )
                .const_label("chain_id", chain_id.to_string()),
            )
            .unwrap(),
            kamu_gql_requests_num_total: IntCounter::with_opts(Opts::new(
                "kamu_gql_requests_num_total",
                "Number of GQL requests executed on Kamu Node",
            ))
            .unwrap(),
            kamu_gql_errors_num_total: IntCounter::with_opts(Opts::new(
                "kamu_gql_errors_num_total",
                "Number of GQL requests executed on Kamu Node that resulted in an error",
            ))
            .unwrap(),
        }
    }

    pub fn register(&self, reg: &prometheus::Registry) -> Result<(), prometheus::Error> {
        reg.register(Box::new(self.evm_rpc_requests_num_total.clone()))?;
        reg.register(Box::new(self.evm_rpc_errors_num_total.clone()))?;
        reg.register(Box::new(self.kamu_gql_requests_num_total.clone()))?;
        reg.register(Box::new(self.kamu_gql_errors_num_total.clone()))?;
        Ok(())
    }
}
