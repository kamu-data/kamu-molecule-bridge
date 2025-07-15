use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::primitives::{Address, U256};
use alloy::providers::DynProvider;
use alloy::rpc::types::Filter;
use alloy_ext::prelude::*;
use color_eyre::eyre;
use color_eyre::eyre::bail;
use kamu_node_api_client::KamuNodeApiClient;
use molecule_contracts::prelude::*;
use molecule_contracts::{IPNFT, Synthesizer, Tokenizer};
use molecule_ipnft::entities::*;
use molecule_ipnft::strategies::IpnftEventProcessingStrategy;
use multisig::services::MultisigResolver;

use crate::config::Config;

pub struct App<'a> {
    config: Config,
    rpc_client: DynProvider,
    multisig_resolver: &'a dyn MultisigResolver,
    kamu_node_api_client: Arc<dyn KamuNodeApiClient>,

    state: AppState,
}

#[derive(Debug, Default)]
struct AppState {
    projects_dataset_offset: u64,

    ipnft_state_map: HashMap<IpnftUid, IpnftState>,
    ipnft_latest_indexed_block_number: u64,

    token_address_ipnft_uid_mapping: HashMap<Address, IpnftUid>,
    tokens_latest_indexed_block_number: u64,
}

#[derive(Debug)]
struct IpnftState {
    ipnft: IpnftEventProjection,
    token: Option<TokenProjection>,
}

#[derive(Debug)]
struct TokenProjection {
    token_address: Address,
    holder_balances: HashMap<Address, U256>,
}

impl<'a> App<'a> {
    pub fn new(
        config: Config,
        rpc_client: DynProvider,
        multisig_resolver: &'a dyn MultisigResolver,
        kamu_node_api_client: Arc<dyn KamuNodeApiClient>,
    ) -> Self {
        Self {
            config,
            rpc_client,
            multisig_resolver,
            kamu_node_api_client,
            state: Default::default(),
        }
    }

    pub async fn run(&mut self) -> eyre::Result<()> {
        let minimal_ipnft_tokenizer_birth_block = self
            .config
            .ipnft_contract_birth_block
            .min(self.config.tokenizer_contract_birth_block);
        let latest_finalized_block_number = self.rpc_client.latest_finalized_block_number().await?;

        let IndexIpnftAndTokenizerContractsResponse {
            ipnft_events,
            tokenizer_events,
        } = self
            .index_ipnft_and_tokenizer_contracts(
                minimal_ipnft_tokenizer_birth_block,
                latest_finalized_block_number,
            )
            .await?;

        let initial_ipnft_event_projection_map = IpnftEventProcessingStrategy.process(ipnft_events);

        for (ipnft_uid, event_projection) in initial_ipnft_event_projection_map {
            self.state.ipnft_state_map.insert(
                ipnft_uid,
                IpnftState {
                    ipnft: event_projection,
                    token: None,
                },
            );
        }

        let ProcessTokenizerEventsResponse {
            ipt_addresses: _,
            minimal_ipt_birth_block: _,
        } = self.process_tokenizer_events(tokenizer_events);

        self.state.ipnft_latest_indexed_block_number = latest_finalized_block_number;

        // TODO: remove
        dbg!(&self.state);

        Ok(())
    }

    async fn index_ipnft_and_tokenizer_contracts(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> eyre::Result<IndexIpnftAndTokenizerContractsResponse> {
        let filter = {
            let addresses = vec![
                self.config.ipnft_contract_address,
                self.config.tokenizer_contract_address,
            ];

            Filter::new()
                .address(addresses)
                .event_signature(HashSet::from_iter([
                    IPNFT::IPNFTMinted::SIGNATURE_HASH,
                    IPNFT::Transfer::SIGNATURE_HASH,
                    Tokenizer::TokensCreated::SIGNATURE_HASH,
                    // NOTE: Backward compatibility, based on:
                    //       https://github.com/moleculeprotocol/IPNFT/blob/main/subgraph/makeAbis.sh
                    Synthesizer::MoleculesCreated::SIGNATURE_HASH,
                ]))
                .from_block(from_block)
                .to_block(to_block)
        };

        let mut ipnft_events = Vec::new();
        let mut tokenizer_events = Vec::new();

        self.rpc_client
            .get_logs_ext(&filter, |logs_chunk| {
                for log in logs_chunk.logs {
                    match log.event_signature_hash() {
                        IPNFT::IPNFTMinted::SIGNATURE_HASH => {
                            let event = IPNFT::IPNFTMinted::decode_log(&log.inner)?;
                            let ipnft_uid = IpnftUid {
                                ipnft_address: event.address,
                                token_id: event.tokenId,
                            };

                            ipnft_events.push(IpnftEvent::Minted(IpnftEventMinted {
                                ipnft_uid,
                                initial_owner: event.owner,
                                symbol: event.symbol.clone(),
                            }));
                        }
                        IPNFT::Transfer::SIGNATURE_HASH => {
                            let event = IPNFT::Transfer::decode_log(&log.inner)?;
                            let ipnft_uid = IpnftUid {
                                ipnft_address: event.address,
                                token_id: event.tokenId,
                            };

                            match (event.from, event.to) {
                                (Address::ZERO, _) => {
                                    // NOTE: Skip as we use higher-level
                                    //       IPNFTMinted event for that
                                }
                                (from, Address::ZERO) => {
                                    ipnft_events.push(IpnftEvent::Burnt(IpnftEventBurnt {
                                        ipnft_uid,
                                        former_owner: from,
                                    }));
                                }
                                (from, to) => {
                                    ipnft_events.push(IpnftEvent::Transfer(IpnftEventTransfer {
                                        ipnft_uid,
                                        from,
                                        to,
                                    }));
                                }
                            }
                        }
                        Tokenizer::TokensCreated::SIGNATURE_HASH => {
                            let event = Tokenizer::TokensCreated::decode_log(&log.inner)?;

                            tokenizer_events.push(TokenizerEvent::TokenCreated(
                                TokenizerEventTokenCreated {
                                    symbol: event.symbol.clone(),
                                    token_id: event.ipnftId,
                                    token_address: event.tokenContract,
                                    birth_block: log.block_number.unwrap_or_default(),
                                },
                            ));
                        }
                        Synthesizer::MoleculesCreated::SIGNATURE_HASH => {
                            let event = Synthesizer::MoleculesCreated::decode_log(&log.inner)?;

                            tokenizer_events.push(TokenizerEvent::TokenCreated(
                                TokenizerEventTokenCreated {
                                    symbol: event.symbol.clone(),
                                    token_id: event.ipnftId,
                                    token_address: event.tokenContract,
                                    birth_block: log.block_number.unwrap_or_default(),
                                },
                            ));
                        }
                        unknown_event_signature_hash => {
                            // TODO: extract error
                            bail!("Unknown event signature hash: {unknown_event_signature_hash}")
                        }
                    }
                }

                Ok(())
            })
            .await?;

        Ok(IndexIpnftAndTokenizerContractsResponse {
            ipnft_events,
            tokenizer_events,
        })
    }

    fn process_tokenizer_events(
        &mut self,
        tokenizer_events: Vec<TokenizerEvent>,
    ) -> ProcessTokenizerEventsResponse {
        let mut ipt_addresses = Vec::with_capacity(tokenizer_events.capacity());
        let mut minimal_ipt_birth_block = 0;

        for event in tokenizer_events {
            match event {
                TokenizerEvent::TokenCreated(TokenizerEventTokenCreated {
                    token_id,
                    token_address,
                    symbol,
                    birth_block,
                }) => {
                    ipt_addresses.push(token_address);

                    if minimal_ipt_birth_block == 0 {
                        minimal_ipt_birth_block = birth_block;
                    } else {
                        minimal_ipt_birth_block = minimal_ipt_birth_block.min(birth_block);
                    }

                    let maybe_ipnft_state_pair =
                        self.state
                            .ipnft_state_map
                            .iter_mut()
                            .find(|(ipnft_uid, ipnft_state)| {
                                ipnft_uid.token_id == token_id
                                    && ipnft_state.ipnft.symbol.as_ref() == Some(&symbol)
                            });

                    if let Some((ipnft_uid, ipnft_state)) = maybe_ipnft_state_pair {
                        ipnft_state.token = Some(TokenProjection {
                            token_address,
                            // NOTE: Will be populated later
                            holder_balances: HashMap::new(),
                        });

                        self.state
                            .token_address_ipnft_uid_mapping
                            .insert(token_address, *ipnft_uid);
                    } else {
                        // TODO: warning message -- token without IPNFT
                    }
                }
            }
        }

        ProcessTokenizerEventsResponse {
            ipt_addresses,
            minimal_ipt_birth_block,
        }
    }
}

struct IndexIpnftAndTokenizerContractsResponse {
    ipnft_events: Vec<IpnftEvent>,
    tokenizer_events: Vec<TokenizerEvent>,
}

struct ProcessTokenizerEventsResponse {
    ipt_addresses: Vec<Address>,
    minimal_ipt_birth_block: u64,
}
