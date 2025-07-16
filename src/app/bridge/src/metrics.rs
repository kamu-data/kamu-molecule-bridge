pub struct BridgeMetrics {
    pub rpc_queries_num: prometheus::IntCounter,
}

impl BridgeMetrics {
    pub fn new(chain_id: u64) -> Self {
        use prometheus::*;

        Self {
            rpc_queries_num: IntCounter::with_opts(
                Opts::new("rpc_queries_num", "Blockchain node RPC queries executed")
                    .const_label("chain_id", chain_id.to_string()),
            )
            .unwrap(),
        }
    }

    pub fn register(&self, reg: &prometheus::Registry) -> Result<(), prometheus::Error> {
        reg.register(Box::new(self.rpc_queries_num.clone()))?;
        Ok(())
    }
}
