use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::primitives::Address;
use alloy::providers::{DynProvider, Provider};
use async_trait::async_trait;
use color_eyre::eyre::{self, bail};
use multisig::services::MultisigResolver;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Default)]
struct State {
    cache_multisig_address_owners_mapping: HashMap<Address, Option<HashSet<Address>>>,
}

/// Safe Wallet Service for interacting with Safe Transaction Service API
#[derive(Clone)]
pub struct SafeWalletApiService {
    api_base_url: &'static str,
    http_client: Client,
    rpc_client: DynProvider,
    state: Arc<RwLock<State>>,
}

impl SafeWalletApiService {
    pub fn new_from_chain_id(chain_id: u64, rpc_client: DynProvider) -> eyre::Result<Self> {
        let api_base_url = Self::get_safe_api_base_url(chain_id)?;
        let http_client = Client::new();

        Ok(Self {
            api_base_url,
            http_client,
            rpc_client,
            state: Default::default(),
        })
    }

    fn get_safe_api_base_url(chain_id: u64) -> eyre::Result<&'static str> {
        // Doc: list of all networks
        //      https://docs.safe.global/advanced/smart-account-supported-networks?service=Transaction+Service

        match chain_id {
            // https://docs.safe.global/core-api/transaction-service-reference/mainnet
            1 => Ok("https://safe-transaction-mainnet.safe.global"),
            // https://docs.safe.global/core-api/transaction-service-reference/sepolia
            11_155_111 => Ok("https://safe-transaction-sepolia.safe.global"),

            _ => bail!("Unsupported network with chain ID: {chain_id}"),
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(address = %address))]
    async fn is_contract(&self, address: Address) -> eyre::Result<bool> {
        let code = self.rpc_client.get_code_at(address).await?;
        Ok(!code.is_empty())
    }
}

#[async_trait]
impl MultisigResolver for SafeWalletApiService {
    async fn get_multisig_owners(
        &self,
        address: Address,
    ) -> eyre::Result<Option<HashSet<Address>>> {
        {
            let readable_state = self.state.read().await;
            if let Some(cached_result) = readable_state
                .cache_multisig_address_owners_mapping
                .get(&address)
            {
                return Ok(cached_result.clone());
            }
        }

        // Cheap call (blockchain)
        if !self.is_contract(address).await? {
            let mut writable_state = self.state.write().await;
            writable_state
                .cache_multisig_address_owners_mapping
                .insert(address, None);

            return Ok(None);
        }

        // TODO: use autogenerated REST API client?
        let api_endpoint = format!("{}/api/v1/safes/{address}/", self.api_base_url);

        // Expensive call to Safe Transaction API (HTTP)
        // TODO: retry logic?
        let response = self.http_client.get(&api_endpoint).send().await?;
        match response.status() {
            StatusCode::OK => {
                // Continue processing
            }
            StatusCode::NOT_FOUND => {
                let mut writable_state = self.state.write().await;
                writable_state
                    .cache_multisig_address_owners_mapping
                    .insert(address, None);

                return Ok(None);
            }
            unexpected => bail!("Unexpected status code: {unexpected}"),
        }

        // We don't need the full structure definition
        #[derive(Debug, Serialize, Deserialize)]
        struct SafeInfoResponseLike {
            pub address: Address,
            pub owners: Vec<Address>,
        }

        let response: SafeInfoResponseLike = response.json().await?;
        assert_eq!(address, response.address);

        let owners = response.owners.into_iter().collect::<HashSet<_>>();

        let mut writable_state = self.state.write().await;
        writable_state
            .cache_multisig_address_owners_mapping
            .insert(address, Some(owners.clone()));

        Ok(Some(owners))
    }
}
