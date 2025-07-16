use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::primitives::{Address, U256};
use alloy::providers::DynProvider;
use alloy::rpc::types::Filter;
use alloy_ext::prelude::*;
use color_eyre::eyre;
use color_eyre::eyre::{ContextCompat, bail};
use kamu_node_api_client::*;
use molecule_contracts::prelude::*;
use molecule_contracts::{IPNFT, IPToken, Synthesizer, Tokenizer};
use molecule_ipnft::entities::*;
use molecule_ipnft::strategies::IpnftEventProcessingStrategy;
use multisig::services::MultisigResolver;

use crate::config::Config;

pub struct App {
    config: Config,
    rpc_client: DynProvider,
    multisig_resolver: Arc<dyn MultisigResolver>,
    kamu_node_api_client: Arc<dyn KamuNodeApiClient>,

    state: AppState,
}

#[derive(Debug, Default)]
struct AppState {
    #[expect(dead_code)]
    projects_dataset_offset: u64,

    ipnft_state_map: HashMap<IpnftUid, IpnftState>,
    ipnft_latest_indexed_block_number: u64,

    token_address_ipnft_uid_mapping: HashMap<Address, IpnftUid>,
    tokens_latest_indexed_block_number: u64,
}

#[derive(Debug)]
struct IpnftState {
    ipnft: IpnftEventProjection,
    #[expect(dead_code)]
    project: Option<ProjectProjection>,
    token: Option<TokenProjection>,
}

#[expect(dead_code)]
#[derive(Debug)]
struct ProjectProjection {
    entry: MoleculeProjectEntry,
    actual_files_map: HashMap<DatasetID, VersionedFileEntryWithMoleculeAccessLevel>,
    removed_files_map: HashMap<DatasetID, VersionedFileEntryWithMoleculeAccessLevel>,
}

#[derive(Debug)]
struct VersionedFileEntryWithMoleculeAccessLevel {
    entry: VersionedFileEntry,
    molecule_access_level: MoleculeAccessLevel,
}

#[derive(Debug)]
struct TokenProjection {
    token_address: Address,
    holder_balances: HashMap<Address, U256>,
}

impl App {
    pub fn new(
        config: Config,
        rpc_client: DynProvider,
        multisig_resolver: Arc<dyn MultisigResolver>,
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

    #[tracing::instrument(level = "info", skip_all)]
    pub async fn run(&mut self) -> eyre::Result<()> {
        let latest_finalized_block_number = self.rpc_client.latest_finalized_block_number().await?;

        self.initial_indexing(latest_finalized_block_number).await?;
        self.initial_projects_loading().await?;

        tracing::info!("App state: {:#?}", self.state);

        Ok(())
    }

    async fn initial_indexing(&mut self, to_block: u64) -> eyre::Result<()> {
        let minimal_ipnft_tokenizer_birth_block = self
            .config
            .ipnft_contract_birth_block
            .min(self.config.tokenizer_contract_birth_block);

        let IndexIpnftAndTokenizerContractsResponse {
            ipnft_events,
            tokenizer_events,
        } = self
            .index_ipnft_and_tokenizer_contracts(minimal_ipnft_tokenizer_birth_block, to_block)
            .await?;

        let initial_ipnft_event_projection_map = IpnftEventProcessingStrategy.process(ipnft_events);

        for (ipnft_uid, event_projection) in initial_ipnft_event_projection_map {
            self.state.ipnft_state_map.insert(
                ipnft_uid,
                IpnftState {
                    ipnft: event_projection,
                    project: None,
                    token: None,
                },
            );
        }

        let ProcessTokenizerEventsResponse {
            minimal_ipt_birth_block,
        } = self.process_tokenizer_events(tokenizer_events);

        self.state.ipnft_latest_indexed_block_number = to_block;

        let token_transfer_events = self.index_tokens(minimal_ipt_birth_block, to_block).await?;
        self.process_token_transfer_events(token_transfer_events)?;

        self.state.tokens_latest_indexed_block_number = to_block;

        Ok(())
    }

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            from_block = from_block,
            to_block = to_block,
            diff = to_block - from_block,
        )
    )]
    async fn index_ipnft_and_tokenizer_contracts(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> eyre::Result<IndexIpnftAndTokenizerContractsResponse> {
        debug_assert!(to_block >= from_block);

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
            .get_logs_ext(&filter, &mut |logs_chunk| {
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

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            from_block = from_block,
            to_block = to_block,
            diff = to_block - from_block,
        )
    )]
    async fn index_tokens(
        &mut self,
        from_block: u64,
        to_block: u64,
    ) -> eyre::Result<Vec<IptEventTransfer>> {
        debug_assert!(to_block >= from_block);

        let filter = {
            let addresses = self
                .state
                .token_address_ipnft_uid_mapping
                .keys()
                .copied()
                .collect::<Vec<_>>();

            if addresses.is_empty() {
                // TODO: warning
                return Ok(Vec::new());
            }

            Filter::new()
                .address(addresses)
                .event_signature(IPToken::Transfer::SIGNATURE_HASH)
                .from_block(from_block)
                .to_block(to_block)
        };

        let mut events = Vec::new();

        self.rpc_client
            .get_logs_ext(&filter, &mut |logs_chunk| {
                for log in logs_chunk.logs {
                    match log.event_signature_hash() {
                        IPToken::Transfer::SIGNATURE_HASH => {
                            let event = IPToken::Transfer::decode_log(&log.inner)?;

                            events.push(IptEventTransfer {
                                token_address: event.address,
                                from: event.from,
                                to: event.to,
                                value: event.value,
                            });
                        }
                        unknown_event_signature_hash => {
                            bail!("Unknown event signature hash: {unknown_event_signature_hash}")
                        }
                    }
                }

                Ok(())
            })
            .await?;

        Ok(events)
    }

    fn process_tokenizer_events(
        &mut self,
        tokenizer_events: Vec<TokenizerEvent>,
    ) -> ProcessTokenizerEventsResponse {
        let mut minimal_birth_block = 0;

        for event in tokenizer_events {
            match event {
                TokenizerEvent::TokenCreated(TokenizerEventTokenCreated {
                    token_id,
                    token_address,
                    symbol,
                    birth_block,
                }) => {
                    let maybe_ipnft_state_pair =
                        self.state
                            .ipnft_state_map
                            .iter_mut()
                            .find(|(ipnft_uid, ipnft_state)| {
                                ipnft_uid.token_id == token_id
                                    && ipnft_state.ipnft.symbol.as_ref() == Some(&symbol)
                            });

                    let Some((ipnft_uid, ipnft_state)) = maybe_ipnft_state_pair else {
                        // TODO: warning message -- token without IPNFT
                        continue;
                    };

                    ipnft_state.token = Some(TokenProjection {
                        token_address,
                        // NOTE: Will be populated later
                        holder_balances: HashMap::new(),
                    });

                    self.state
                        .token_address_ipnft_uid_mapping
                        .insert(token_address, *ipnft_uid);

                    if minimal_birth_block == 0 {
                        minimal_birth_block = birth_block;
                    } else {
                        minimal_birth_block = minimal_birth_block.min(birth_block);
                    }
                }
            }
        }

        ProcessTokenizerEventsResponse {
            minimal_ipt_birth_block: minimal_birth_block,
        }
    }

    fn process_token_transfer_events(&mut self, events: Vec<IptEventTransfer>) -> eyre::Result<()> {
        for event in events {
            let Some(ipnft_uid) = self
                .state
                .token_address_ipnft_uid_mapping
                .get(&event.token_address)
            else {
                // TODO: warning message -- token without IPNFT
                continue;
            };

            let ipnft_state = self
                .state
                .ipnft_state_map
                .get_mut(ipnft_uid)
                .wrap_err_with(|| format!("IPNFT should be present: '{ipnft_uid}'"))?;
            let token_projection = ipnft_state
                .token
                .as_mut()
                .wrap_err_with(|| format!("Token should be present: '{ipnft_uid}'"))?;

            debug_assert_eq!(token_projection.token_address, event.token_address);

            if event.from != Address::ZERO {
                let balance = token_projection
                    .holder_balances
                    .entry(event.from)
                    .or_default();
                *balance -= event.value;
            }

            if event.to != Address::ZERO {
                let balance = token_projection
                    .holder_balances
                    .entry(event.to)
                    .or_default();
                *balance += event.value;
            }
        }

        Ok(())
    }

    #[expect(dead_code)]
    async fn initial_projects_loading(&mut self) -> eyre::Result<()> {
        let all_projects_entries = self
            .kamu_node_api_client
            .get_molecule_project_entries(0) // NOTE: full scan
            .await?;
        let data_room_dataset_ids = all_projects_entries
            .iter()
            .map(|project| DataRoomDatasetIdWithOffset {
                dataset_id: project.data_room_dataset_id.clone(),
                offset: 0, // NOTE: full scan
            })
            .collect();
        let mut versioned_files_entries_map = self
            .kamu_node_api_client
            .get_versioned_files_entries_by_data_rooms(data_room_dataset_ids)
            .await?;
        let versioned_file_dataset_ids =
            versioned_files_entries_map
                .values()
                .fold(Vec::new(), |mut acc, entries| {
                    acc.extend(entries.added_entities.keys().cloned());
                    acc.extend(entries.removed_entities.keys().cloned());
                    acc
                });
        let molecule_access_levels_map = self
            .kamu_node_api_client
            .get_latest_molecule_access_levels_by_dataset_ids(versioned_file_dataset_ids)
            .await?;

        let mut projects_dataset_offset = 0;
        for project_entry in all_projects_entries {
            projects_dataset_offset = project_entry.offset;

            let Some(ipnft_state) = self.state.ipnft_state_map.get_mut(&project_entry.ipnft_uid)
            else {
                // TODO: warning message -- project without IPNFT in blockchain
                continue;
            };

            let versioned_files_entries = versioned_files_entries_map
                // NOTE: try to extract a value from the map
                .remove(&project_entry.data_room_dataset_id)
                .unwrap_or_default();
            // TODO: extract impl From<...>
            let actual_files_map = versioned_files_entries
                .added_entities
                .into_iter()
                .map(|(dataset_id, file_entry)| {
                    let molecule_access_level =
                        if let Some(value) = molecule_access_levels_map.get(&dataset_id) {
                            *value
                        } else {
                            todo!();
                        };
                    (
                        dataset_id,
                        VersionedFileEntryWithMoleculeAccessLevel {
                            entry: file_entry,
                            molecule_access_level,
                        },
                    )
                })
                .collect();
            let removed_files_map = versioned_files_entries
                .removed_entities
                .into_iter()
                .map(|(dataset_id, file_entry)| {
                    let molecule_access_level =
                        if let Some(value) = molecule_access_levels_map.get(&dataset_id) {
                            *value
                        } else {
                            todo!();
                        };
                    (
                        dataset_id,
                        VersionedFileEntryWithMoleculeAccessLevel {
                            entry: file_entry,
                            molecule_access_level,
                        },
                    )
                })
                .collect();

            ipnft_state.project = Some(ProjectProjection {
                entry: project_entry,
                actual_files_map,
                removed_files_map,
            });
        }

        self.state.projects_dataset_offset = projects_dataset_offset;

        Ok(())
    }
}

struct IndexIpnftAndTokenizerContractsResponse {
    ipnft_events: Vec<IpnftEvent>,
    tokenizer_events: Vec<TokenizerEvent>,
}

struct ProcessTokenizerEventsResponse {
    minimal_ipt_birth_block: u64,
}
