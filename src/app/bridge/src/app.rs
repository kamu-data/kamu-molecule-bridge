use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::primitives::{Address, Log, U256};
use alloy::providers::DynProvider;
use alloy_ext::prelude::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use color_eyre::eyre;
use color_eyre::eyre::{ContextCompat, bail};
use kamu_node_api_client::*;
use molecule_contracts::prelude::*;
use molecule_contracts::{IPNFT, IPToken, Safe, Synthesizer, Tokenizer, safe};
use molecule_ipnft::entities::*;
use molecule_ipnft::strategies::IpnftEventProcessingStrategy;
use multisig::services::MultisigResolver;
use serde::{Serialize, Serializer};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::Instrument as _;

use crate::config::Config;
use crate::http_server;
use crate::http_server::{HttpServeFuture, StateRequester};
use crate::metrics::BridgeMetrics;

// TODO: Update when it's agreed
const IPT_ACCESS_THRESHOLD: U256 = U256::ZERO;

pub struct App {
    config: Config,
    ignore_projects_ipnft_uids: HashSet<String>,

    rpc_client: DynProvider,
    multisig_resolver: Arc<dyn MultisigResolver>,
    kamu_node_api_client: Arc<dyn KamuNodeApiClient>,

    #[expect(dead_code)]
    metrics: BridgeMetrics,
    metrics_registry: prometheus::Registry,

    state: Arc<RwLock<AppState>>,
}

#[derive(Debug, Default, Serialize)]
pub struct AppState {
    projects_dataset_offset: Option<u64>,

    ipnft_state_map: HashMap<IpnftUid, IpnftState>,
    latest_indexed_block_number: u64,

    token_address_ipnft_uid_mapping: HashMap<Address, IpnftUid>,
    tokens_latest_indexed_block_number: u64,

    molecule_projects_last_requested_at: Option<DateTime<Utc>>,
    multisig: HashMap<Address, Option<MultisigState>>,
    access_changes: HashMap<DateTime<Utc>, AccessChanges>,
}

#[derive(Debug, Serialize)]
struct AccessChanges {
    reason: String,
    operations: Vec<AccountDatasetRelationOperation>,
}

#[async_trait]
impl StateRequester for RwLock<AppState> {
    async fn request_as_json(&self) -> Value {
        let readable_state = self.read().await;
        serde_json::to_value(&*readable_state).unwrap()
    }
}

#[derive(Debug, Serialize)]
struct IpnftState {
    ipnft: IpnftEventProjection,
    project: Option<ProjectProjection>,
    token: Option<TokenProjection>,
}

#[derive(Debug, Serialize)]
struct MultisigState {
    current_owners: HashSet<Address>,
    former_owners: HashSet<Address>,
}

#[derive(Debug, Serialize)]
struct ProjectProjection {
    entry: MoleculeProjectEntry,
    latest_data_room_offset: u64,
    actual_files_map: HashMap<DatasetID, VersionedFileEntryWithMoleculeAccessLevel>,
    removed_files_map: HashMap<DatasetID, VersionedFileEntry>,
}

#[derive(Debug, Serialize)]
struct VersionedFileEntryWithMoleculeAccessLevel {
    entry: VersionedFileEntry,
    molecule_access_level: MoleculeAccessLevel,
}

#[derive(Debug, Serialize)]
struct TokenProjection {
    token_address: Address,
    #[serde(serialize_with = "serialize_hashmap_values_as_string")]
    holder_balances: HashMap<Address, U256>,
}

fn serialize_hashmap_values_as_string<S>(
    hash_map: &HashMap<Address, U256>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeMap;

    let mut map = serializer.serialize_map(Some(hash_map.len()))?;
    for (k, v) in hash_map {
        map.serialize_entry(k, &v.to_string())?;
    }
    map.end()
}

impl App {
    pub fn new(
        config: Config,
        rpc_client: DynProvider,
        multisig_resolver: Arc<dyn MultisigResolver>,
        kamu_node_api_client: Arc<dyn KamuNodeApiClient>,
        metrics: BridgeMetrics,
        metrics_registry: prometheus::Registry,
    ) -> Self {
        let ignore_projects_ipnft_uids = config
            .ignore_projects_ipnft_uids
            .clone()
            .unwrap_or_default();

        Self {
            config,
            ignore_projects_ipnft_uids,
            rpc_client,
            multisig_resolver,
            kamu_node_api_client,
            metrics,
            metrics_registry,
            state: Default::default(),
        }
    }

    /// Loads the state and returns it without making any modifications to permissions
    pub async fn get_state(mut self) -> eyre::Result<AppState> {
        self.init_state().await
    }

    /// Initializes the state and enters a continuous indexing loop
    pub async fn run<F>(&mut self, shutdown_requested: F) -> eyre::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        // Initialization
        let http_serve_future = self
            .build_http_server(self.metrics_registry.clone())
            .await?;
        let http_server = http_serve_future.with_graceful_shutdown(shutdown_requested);

        // Asynchronous execution: HTTP server and indexing
        tokio::select! {
            res = http_server => { res.map_err(Into::into) },
            res = self.main() => { res },
        }
    }

    async fn build_http_server(
        &mut self,
        metrics_registry: prometheus::Registry,
    ) -> eyre::Result<HttpServeFuture> {
        let (http_server, local_addr) = http_server::build(
            self.config.http_address,
            self.config.http_port,
            metrics_registry,
            self.state.clone(),
        )
        .await?;

        tracing::info!("HTTP API is listening on {local_addr}");

        Ok(http_server)
    }

    async fn main(&mut self) -> eyre::Result<()> {
        // NOTE: In OTEL we should not have traces that last more than a few seconds,
        // so we break up the infinite main loop into spans attached to individual iterations,
        // and using `root_span!()` ensures they are assigned a top-level `trace_id`.

        self.init()
            .instrument(observability::tracing::root_span!("App::init"))
            .await?;

        let iteration_delay =
            std::time::Duration::from_secs(self.config.indexing_delay_between_iterations_in_secs);

        loop {
            tokio::time::sleep(iteration_delay).await;

            self.update()
                .instrument(observability::tracing::root_span!("App::update"))
                .await?;
        }
    }

    async fn init(&mut self) -> eyre::Result<()> {
        let mut initial_app_state = self.init_state().await?;

        self.initial_access_applying(&mut initial_app_state).await?;

        {
            let mut writable_state = self.state.write().await;
            *writable_state = initial_app_state;
        }

        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn init_state(&mut self) -> eyre::Result<AppState> {
        let latest_finalized_block_number = self.rpc_client.latest_finalized_block_number().await?;

        let minimal_ipnft_or_tokenizer_birth_block_minus_one = self
            .config
            .ipnft_contract_birth_block
            .min(self.config.tokenizer_contract_birth_block)
            - 1;

        let mut initial_app_state = AppState {
            latest_indexed_block_number: minimal_ipnft_or_tokenizer_birth_block_minus_one,
            ..Default::default()
        };

        self.indexing(&mut initial_app_state, latest_finalized_block_number)
            .await?;

        self.load_molecule_projects(&mut initial_app_state).await?;

        Ok(initial_app_state)
    }

    async fn update(&mut self) -> eyre::Result<()> {
        tracing::info!("Performing update loop iteration");

        let latest_finalized_block_number = self.rpc_client.latest_finalized_block_number().await?;

        let mut writable_state = self.state.clone().write_owned().await;

        let next_block_for_indexing = writable_state.latest_indexed_block_number + 1;
        if latest_finalized_block_number <= next_block_for_indexing {
            tracing::info!(
                "Skip update iteration as there are no new blocks to index: {latest_finalized_block_number} <= {next_block_for_indexing}"
            );
            return Ok(());
        }

        let IndexingResponse {
            mut ipnft_changes_map,
        } = self
            .indexing(&mut writable_state, latest_finalized_block_number)
            .await?;

        let elapsed_secs: u64 = {
            let last_requested_at = writable_state
                .molecule_projects_last_requested_at
                .unwrap_or_default();
            (Utc::now() - last_requested_at).num_seconds().try_into()?
        };
        let interval = self.config.molecule_projects_loading_interval_in_secs;

        if elapsed_secs >= interval {
            let versioned_file_changes_per_projects =
                self.load_molecule_projects(&mut writable_state).await?;

            for (ipnft_uid, changed_files) in versioned_file_changes_per_projects {
                let ipnft_changes = ipnft_changes_map.entry(ipnft_uid).or_default();
                ipnft_changes.changed_files = changed_files;
            }

            writable_state.molecule_projects_last_requested_at = Some(Utc::now());
        }

        self.interval_access_applying(
            &mut writable_state,
            ipnft_changes_map,
            next_block_for_indexing,
        )
        .await?;

        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all, fields(to_block = to_block))]
    async fn indexing(
        &mut self,
        app_state: &mut AppState,
        to_block: u64,
    ) -> eyre::Result<IndexingResponse> {
        let IndexIpnftAndTokenizerContractsResponse {
            ipnft_events,
            tokenizer_events,
        } = self
            .index_ipnft_and_tokenizer_contracts(
                app_state.latest_indexed_block_number + 1,
                to_block,
            )
            .await?;

        let initial_ipnft_event_projection_map = IpnftEventProcessingStrategy.process(ipnft_events);
        for (ipnft_uid, event_projection) in &initial_ipnft_event_projection_map {
            let mut just_created = false;
            let ipnft_state = app_state
                .ipnft_state_map
                .entry(*ipnft_uid)
                .or_insert_with(|| {
                    just_created = true;
                    IpnftState {
                        ipnft: event_projection.clone(),
                        project: None,
                        token: None,
                    }
                });
            // NOTE: No need to sync events the first time.
            if !just_created {
                IpnftEventProcessingStrategy.synchronize_ipnft_event_projections(
                    &mut ipnft_state.ipnft,
                    event_projection.clone(),
                );
            }
        }

        let IndexMultisigSafesResponse {
            changed_ipnft_multisig_owners,
        } = self
            .index_multisig_safes(
                app_state,
                app_state.latest_indexed_block_number + 1,
                to_block,
            )
            .await?;

        app_state.latest_indexed_block_number = to_block;

        let ProcessTokenizerEventsResponse {
            minimal_ipt_birth_block,
        } = self.process_tokenizer_events(app_state, tokenizer_events);

        let from_block = if app_state.tokens_latest_indexed_block_number == 0 {
            minimal_ipt_birth_block
        } else {
            app_state.tokens_latest_indexed_block_number + 1
        };
        let token_transfer_events = self.index_tokens(app_state, from_block, to_block).await?;
        let ProcessTokenTransferEventsResponse {
            participating_holders_balances,
        } = self.process_token_transfer_events(app_state, token_transfer_events)?;

        app_state.tokens_latest_indexed_block_number = to_block;

        // Populate IPNFT blockchain changes:
        let mut ipnft_changes_map: HashMap<IpnftUid, IpnftChanges> = HashMap::new();
        // 1. From IPNFT contract
        for (ipnft_uid, ipnft_event_projection) in initial_ipnft_event_projection_map {
            let ipnft_change = ipnft_changes_map.entry(ipnft_uid).or_default();

            if ipnft_event_projection.minted && ipnft_event_projection.burnt {
                // NOTE: IPNFT was burned before we could give access to anyone.
                //       So there's no need to revoke access from anyone as well.
                tracing::info!("Skip burnt IPNFT: '{ipnft_uid}'");
                ipnft_change.minted_and_burnt = true;
                continue;
            }

            ipnft_change.owner_changes.current_owner = ipnft_event_projection.current_owner;
            ipnft_change.owner_changes.former_owner = ipnft_event_projection.former_owner;
        }
        // 2. From IPToken contracts
        for (ipnft_uid, holders_balances_changes) in participating_holders_balances {
            let ipnft_change = ipnft_changes_map.entry(ipnft_uid).or_default();

            if ipnft_change.minted_and_burnt {
                continue;
            }

            ipnft_change.holder_balances_changes = holders_balances_changes;
        }
        // 3. From multisig changes
        for (ipnft_uid, owner) in changed_ipnft_multisig_owners {
            let ipnft_change = ipnft_changes_map.entry(ipnft_uid).or_default();

            // If the current owner changes, we will request new data from the multisig state if needed.
            if ipnft_change.owner_changes.current_owner.is_none() {
                // If there is no owner change, we need to trigger new permissions [re]grant in IPNFT.
                ipnft_change.owner_changes.current_owner = Some(owner);
            }
        }

        Ok(IndexingResponse { ipnft_changes_map })
    }

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            from_block = from_block,
            to_block = to_block,
            diff = to_block.checked_sub(from_block),
        )
    )]
    async fn index_ipnft_and_tokenizer_contracts(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> eyre::Result<IndexIpnftAndTokenizerContractsResponse> {
        let event_signatures = HashSet::from_iter([
            IPNFT::IPNFTMinted::SIGNATURE_HASH,
            IPNFT::Transfer::SIGNATURE_HASH,
            Tokenizer::TokensCreated::SIGNATURE_HASH,
            // NOTE: Backward compatibility, based on:
            //       https://github.com/moleculeprotocol/IPNFT/blob/main/subgraph/makeAbis.sh
            Synthesizer::MoleculesCreated::SIGNATURE_HASH,
        ]);

        let mut ipnft_events = Vec::new();
        let mut tokenizer_events = Vec::new();

        self.rpc_client
            .get_logs_ext(
                vec![
                    self.config.ipnft_contract_address,
                    self.config.tokenizer_contract_address,
                ],
                event_signatures,
                from_block,
                to_block,
                &mut |logs_chunk| {
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
                                        ipnft_events.push(IpnftEvent::Transfer(
                                            IpnftEventTransfer {
                                                ipnft_uid,
                                                from,
                                                to,
                                            },
                                        ));
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
                                bail!(
                                    "Unknown event signature hash: {unknown_event_signature_hash}"
                                )
                            }
                        }
                    }

                    Ok(())
                },
            )
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
            diff = to_block.checked_sub(from_block),
        )
    )]
    async fn index_multisig_safes(
        &self,
        app_state: &mut AppState,
        from_block: u64,
        to_block: u64,
    ) -> eyre::Result<IndexMultisigSafesResponse> {
        let multisigs = app_state.multisig.keys().copied().collect::<Vec<_>>();

        if multisigs.is_empty() {
            return Ok(IndexMultisigSafesResponse::default());
        }

        let mut changed_multisigs = HashSet::new();

        self.rpc_client
            .get_logs_ext(
                multisigs,
                HashSet::from_iter([
                    Safe::AddedOwner::SIGNATURE_HASH,
                    Safe::RemovedOwner::SIGNATURE_HASH,
                ]),
                from_block,
                to_block,
                &mut |logs_chunk| {
                    for log in logs_chunk.logs {
                        let safe_address = log.address();

                        let Some(maybe_multisig_state) = app_state.multisig.get_mut(&safe_address) else {
                            unreachable!();
                        };
                        let Some(multisig_state) = maybe_multisig_state else {
                            unreachable!();
                        };

                        changed_multisigs.insert(safe_address);

                        match log.event_signature_hash() {
                            Safe::AddedOwner::SIGNATURE_HASH => {
                                let added_owner = parse_safe_added_owner_event(&log.inner)?;
                                multisig_state.current_owners.insert(added_owner);
                            }
                            Safe::RemovedOwner::SIGNATURE_HASH => {
                                let removed_owner = parse_safe_removed_owner_event(&log.inner)?;
                                multisig_state.current_owners.remove(&removed_owner);
                                multisig_state.former_owners.insert(removed_owner);
                            }
                            unknown_event_signature_hash => {
                                bail!("Unknown Safe event signature hash: {unknown_event_signature_hash}")
                            }
                        }
                    }

                    Ok(())
                },
            )
            .await?;

        let changed_ipnft_multisig_owners = app_state.ipnft_state_map.iter().fold(
            HashMap::new(),
            |mut acc, (ipnft_uid, ipnft_state)| {
                if let Some(owner) = ipnft_state.ipnft.current_owner
                    && changed_multisigs.contains(&owner)
                {
                    acc.insert(*ipnft_uid, owner);
                }
                acc
            },
        );

        Ok(IndexMultisigSafesResponse {
            changed_ipnft_multisig_owners,
        })
    }

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            from_block = from_block,
            to_block = to_block,
            diff = to_block.checked_sub(from_block),
        )
    )]
    async fn index_tokens(
        &mut self,
        app_state: &AppState,
        from_block: u64,
        to_block: u64,
    ) -> eyre::Result<Vec<IptEventTransfer>> {
        let token_addresses = app_state
            .token_address_ipnft_uid_mapping
            .keys()
            .copied()
            .collect::<Vec<_>>();
        if token_addresses.is_empty() {
            tracing::warn!("No tokens to index");
            return Ok(Vec::new());
        }

        let event_signatures = HashSet::from_iter([IPToken::Transfer::SIGNATURE_HASH]);

        let mut events = Vec::new();

        self.rpc_client
            .get_logs_ext(
                token_addresses,
                event_signatures,
                from_block,
                to_block,
                &mut |logs_chunk| {
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
                                bail!(
                                    "Unknown event signature hash: {unknown_event_signature_hash}"
                                )
                            }
                        }
                    }

                    Ok(())
                },
            )
            .await?;

        Ok(events)
    }

    fn process_tokenizer_events(
        &mut self,
        app_state: &mut AppState,
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
                        app_state
                            .ipnft_state_map
                            .iter_mut()
                            .find(|(ipnft_uid, ipnft_state)| {
                                ipnft_uid.token_id == token_id
                                    && ipnft_state.ipnft.symbol.as_ref() == Some(&symbol)
                            });

                    let Some((ipnft_uid, ipnft_state)) = maybe_ipnft_state_pair else {
                        tracing::warn!(
                            "Skip '{symbol}' ({token_id}/{token_address}) token as there is no corresponding IPNFT"
                        );
                        continue;
                    };

                    debug_assert!(ipnft_state.token.is_none());

                    ipnft_state.token = Some(TokenProjection {
                        token_address,
                        // NOTE: Will be populated later
                        holder_balances: HashMap::new(),
                    });

                    app_state
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

    fn process_token_transfer_events(
        &mut self,
        app_state: &mut AppState,
        events: Vec<IptEventTransfer>,
    ) -> eyre::Result<ProcessTokenTransferEventsResponse> {
        let mut participating_holders_balances = HashMap::<IpnftUid, HashMap<Address, U256>>::new();

        for event in events {
            let Some(ipnft_uid) = app_state
                .token_address_ipnft_uid_mapping
                .get(&event.token_address)
            else {
                tracing::warn!(
                    "Skip event processing as token ({}) has no IPNFT",
                    event.token_address
                );
                continue;
            };

            let ipnft_state = app_state
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

                let changed_balances = participating_holders_balances
                    .entry(*ipnft_uid)
                    .or_default();
                changed_balances.insert(event.from, *balance);
            }

            if event.to != Address::ZERO {
                let balance = token_projection
                    .holder_balances
                    .entry(event.to)
                    .or_default();
                *balance += event.value;

                let changed_balances = participating_holders_balances
                    .entry(*ipnft_uid)
                    .or_default();
                changed_balances.insert(event.from, *balance);
            }
        }

        Ok(ProcessTokenTransferEventsResponse {
            participating_holders_balances,
        })
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn load_molecule_projects(
        &mut self,
        app_state: &mut AppState,
    ) -> eyre::Result<ChangedVersionedFilePerProjectMap> {
        // Project updates are based on several principles:
        // - To query new dataset entries, we use the Ledger storage strategy advantages: for new changes,
        //   we just need a larger offset.
        // - In case of checking molecule_access_level changes, we also request information about existing files.

        // I. Preparations.
        let mut detected_changes_map = HashMap::new();

        // First, check for new files in known projects (if any).
        let existing_projects = app_state
            .ipnft_state_map
            .values_mut()
            .filter_map(|ipnft_state| ipnft_state.project.as_mut())
            .collect::<Vec<_>>();
        let existing_data_room_dataset_ids_with_offsets = existing_projects
            .iter()
            .map(|project| DataRoomDatasetIdWithOffset {
                dataset_id: project.entry.data_room_dataset_id.clone(),
                offset: project.latest_data_room_offset + 1,
            })
            .collect::<Vec<_>>();

        // Second, check for new allowlisted projects.
        let new_projects_entries = self
            .kamu_node_api_client
            .get_molecule_project_entries(
                app_state
                    .projects_dataset_offset
                    .map(|off| off + 1)
                    .unwrap_or(0),
                &self.ignore_projects_ipnft_uids,
            ) // NOTE: only new allowlisted projects
            .await?;
        let new_data_room_dataset_ids_with_offsets = new_projects_entries
            .iter()
            .map(|project| DataRoomDatasetIdWithOffset {
                dataset_id: project.data_room_dataset_id.clone(),
                offset: 0, // NOTE: full scan
            })
            .collect::<Vec<_>>();

        // Combine data for batch requests.
        let data_room_dataset_ids_with_offsets = {
            let mut ids = Vec::with_capacity(
                new_data_room_dataset_ids_with_offsets.len()
                    + existing_data_room_dataset_ids_with_offsets.len(),
            );
            ids.extend(new_data_room_dataset_ids_with_offsets);
            ids.extend(existing_data_room_dataset_ids_with_offsets);
            ids
        };
        let mut versioned_files_entries_map = self
            .kamu_node_api_client
            .get_versioned_files_entries_by_data_rooms(data_room_dataset_ids_with_offsets)
            .await?;

        // Build file "molecule_access_level" mapping:
        let versioned_file_dataset_ids = {
            let added_file_entry_dataset_ids =
                versioned_files_entries_map
                    .values()
                    .fold(Vec::new(), |mut acc, entries| {
                        acc.extend(entries.added_entities.keys().cloned());
                        acc
                    });
            let existing_file_entry_dataset_ids = existing_projects
                .iter()
                .flat_map(|project| project.actual_files_map.keys().cloned())
                .collect::<Vec<_>>();

            let mut ids = Vec::with_capacity(
                added_file_entry_dataset_ids.len() + existing_file_entry_dataset_ids.len(),
            );
            ids.extend(added_file_entry_dataset_ids);
            ids.extend(existing_file_entry_dataset_ids);
            ids
        };
        let molecule_access_levels_map = self
            .kamu_node_api_client
            .get_latest_molecule_access_levels_by_dataset_ids(versioned_file_dataset_ids)
            .await?;

        // II. Process existing projects.
        for existing_project in existing_projects {
            let project_entry = &existing_project.entry;
            let mut detected_changes = Vec::new();

            let _span = tracing::debug_span!(
                "Process existing project",
                symbol = project_entry.symbol,
                ipnft_uid = %project_entry.ipnft_uid
            )
            .entered();

            let Some(versioned_files_entries) = versioned_files_entries_map
                // NOTE: try to extract a value from the map
                .remove(&project_entry.data_room_dataset_id)
            else {
                continue;
            };

            let changed_versioned_files = prepare_changes_based_on_changed_versioned_files_entries(
                &versioned_files_entries,
                &molecule_access_levels_map,
            );
            detected_changes.extend(changed_versioned_files);

            let added_file_entries_map = build_added_file_entries_with_molecule_access_level_map(
                versioned_files_entries.added_entities,
                &molecule_access_levels_map,
            );

            // Update actual files ...
            existing_project.actual_files_map.retain(|dataset_id, _| {
                versioned_files_entries
                    .removed_entities
                    .contains_key(dataset_id)
            });
            existing_project
                .actual_files_map
                .extend(added_file_entries_map);
            // ... (and check if molecule_access_level has changed for existing files), ...
            let changed_versioned_files = prepare_changes_based_on_changed_molecule_access_levels(
                &existing_project.actual_files_map,
                &molecule_access_levels_map,
            );
            detected_changes.extend(changed_versioned_files);

            // ... removed files, ...
            existing_project
                .removed_files_map
                .extend(versioned_files_entries.removed_entities);
            // ... and offset.
            existing_project.latest_data_room_offset =
                versioned_files_entries.latest_data_room_offset;

            if !detected_changes.is_empty() {
                detected_changes_map.insert(project_entry.ipnft_uid, detected_changes);
            }
        }

        // III. Process new projects.
        // NOTE: Projects are sorted, so we can simply assign each new value.
        let mut new_projects_dataset_offset = app_state.projects_dataset_offset;

        for project_entry in new_projects_entries {
            let mut detected_changes = Vec::new();

            let _span = tracing::debug_span!(
                "Process new project",
                symbol = project_entry.symbol,
                ipnft_uid = %project_entry.ipnft_uid
            )
            .entered();

            new_projects_dataset_offset = Some(project_entry.offset);

            let Some(ipnft_state) = app_state.ipnft_state_map.get_mut(&project_entry.ipnft_uid)
            else {
                tracing::info!("Skip project because it's not present in blockchain");
                continue;
            };

            let Some(versioned_files_entries) = versioned_files_entries_map
                // NOTE: try to extract a value from the map
                .remove(&project_entry.data_room_dataset_id)
            else {
                continue;
            };

            let changed_versioned_files = prepare_changes_based_on_changed_versioned_files_entries(
                &versioned_files_entries,
                &molecule_access_levels_map,
            );
            detected_changes.extend(changed_versioned_files);

            let actual_files_map = build_added_file_entries_with_molecule_access_level_map(
                versioned_files_entries.added_entities,
                &molecule_access_levels_map,
            );

            if !detected_changes.is_empty() {
                detected_changes_map.insert(project_entry.ipnft_uid, detected_changes);
            }

            ipnft_state.project = Some(ProjectProjection {
                entry: project_entry,
                latest_data_room_offset: versioned_files_entries.latest_data_room_offset,
                actual_files_map,
                removed_files_map: versioned_files_entries.removed_entities,
            });
        }

        app_state.projects_dataset_offset = new_projects_dataset_offset;

        Ok(detected_changes_map)
    }

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            changed_ipnfts_count = ipnft_changes_map.len(),
        )
    )]
    async fn interval_access_applying(
        &self,
        app_state: &mut AppState,
        ipnft_changes_map: HashMap<IpnftUid, IpnftChanges>,
        to_block: u64,
    ) -> eyre::Result<()> {
        for (ipnft_uid, ipnft_change) in ipnft_changes_map {
            // NOTE: These are post-indexing updates, so all this data must be present.
            let Some(ipnft_state) = app_state.ipnft_state_map.get(&ipnft_uid) else {
                bail!("IPNFT state should be present: {ipnft_uid}")
            };

            let operations = self
                .interval_access_applying_for_ipnft(
                    ipnft_uid,
                    ipnft_state,
                    ipnft_change,
                    &mut app_state.multisig,
                    to_block,
                )
                .await?;

            // Apply operations
            if !operations.is_empty() {
                app_state.access_changes.insert(
                    Utc::now(),
                    AccessChanges {
                        reason: format!(
                            "IPNFT ({:?}/{ipnft_uid}) interval update",
                            ipnft_state.ipnft.symbol
                        ),
                        operations: operations.clone(),
                    },
                );
            }

            self.kamu_node_api_client
                .apply_account_dataset_relations(operations)
                .await?;
        }

        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all, fields(symbol = ipnft_state.ipnft.symbol, ipnft_uid = %ipnft_uid))]
    async fn interval_access_applying_for_ipnft(
        &self,
        ipnft_uid: IpnftUid,
        ipnft_state: &IpnftState,
        ipnft_change: IpnftChanges,
        multisig: &mut HashMap<Address, Option<MultisigState>>,
        to_block: u64,
    ) -> eyre::Result<Vec<AccountDatasetRelationOperation>> {
        if ipnft_change.minted_and_burnt {
            // Nothing to do
            return Ok(Vec::new());
        }

        let Some(project) = &ipnft_state.project else {
            tracing::info!("Skip IPNFT since there is no project created for it");
            debug_assert!(ipnft_change.changed_files.is_empty());
            return Ok(Vec::new());
        };

        // 1. Process new blockchain data.
        let blockchain_based_operations = {
            // Prepare account information
            let mut current_owners = HashSet::new();
            let mut holders = HashSet::new();
            let mut revoke_access_accounts = HashSet::new();

            // TODO: self.get_owners() in parallel for all possible multisig?
            if let Some(current_owner) = ipnft_change.owner_changes.current_owner {
                let GetOwnersResponse {
                    current_owners: new_owners,
                    former_owners,
                } = self.get_owners(current_owner, multisig, to_block).await?;
                current_owners.extend(new_owners);
                revoke_access_accounts.extend(former_owners);
            }
            if let Some(former_owner) = ipnft_change.owner_changes.former_owner {
                let GetOwnersResponse {
                    current_owners: old_owners,
                    former_owners,
                } = self.get_owners(former_owner, multisig, to_block).await?;
                revoke_access_accounts.extend(old_owners);
                revoke_access_accounts.extend(former_owners);
            }

            for (holder, balance) in ipnft_change.holder_balances_changes {
                if balance > IPT_ACCESS_THRESHOLD {
                    holders.insert(holder);
                } else {
                    revoke_access_accounts.insert(holder);
                }
            }

            account_access_sanity_checks(
                &current_owners,
                &mut holders,
                &mut revoke_access_accounts,
            );

            // Create accounts
            let CreateAccountsResponse {
                current_owners_did_pkhs,
                holders_did_pkhs,
                revoke_access_accounts_did_pkh,
            } = self.create_did_pkh_accounts(current_owners, holders, revoke_access_accounts)?;

            let all_accounts_count = current_owners_did_pkhs.len()
                + holders_did_pkhs.len()
                + revoke_access_accounts_did_pkh.len();
            let accounts = {
                let mut v = Vec::with_capacity(all_accounts_count);
                v.extend(current_owners_did_pkhs.clone());
                v.extend(holders_did_pkhs.clone());
                v.extend(revoke_access_accounts_did_pkh.clone());
                v
            };

            self.kamu_node_api_client
                .create_wallet_accounts(accounts)
                .await?;

            let project_dataset_ids = get_project_dataset_ids(project);

            build_operations(
                project_dataset_ids,
                &current_owners_did_pkhs,
                &holders_did_pkhs,
                &revoke_access_accounts_did_pkh,
            )
        };

        // 2. Process the project's changes.
        let operations = if !ipnft_change.changed_files.is_empty() {
            let GetAccountsByIpnftStateResponse {
                current_owners,
                holders,
                revoke_access_accounts,
            } = self
                .get_accounts_by_ipnft_state(ipnft_state, multisig, to_block)
                .await?;
            let CreateAccountsResponse {
                current_owners_did_pkhs,
                holders_did_pkhs,
                revoke_access_accounts_did_pkh,
            } = self.create_did_pkh_accounts(current_owners, holders, revoke_access_accounts)?;

            let mut changed_project_dataset_ids = ProjectDatasetIds::default();

            for changed_file in &ipnft_change.changed_files {
                match changed_file.change {
                    IpnftDataRoomFileChange::Added(molecule_access_level) => {
                        partition_dataset_id_by_molecule_access_level(
                            &changed_file.dataset_id,
                            molecule_access_level,
                            &mut changed_project_dataset_ids.owner_file_dataset_ids,
                            &mut changed_project_dataset_ids.holder_file_dataset_ids,
                        );
                    }
                    IpnftDataRoomFileChange::Removed => {
                        changed_project_dataset_ids
                            .removed_file_dataset_ids
                            .push(&changed_file.dataset_id);
                    }
                    IpnftDataRoomFileChange::MoleculeAccessLevelChanged { from: _, to } => {
                        partition_dataset_id_by_molecule_access_level(
                            &changed_file.dataset_id,
                            to,
                            &mut changed_project_dataset_ids.owner_file_dataset_ids,
                            &mut changed_project_dataset_ids.holder_file_dataset_ids,
                        );
                    }
                }
            }

            let project_based_operations = build_operations(
                changed_project_dataset_ids,
                &current_owners_did_pkhs,
                &holders_did_pkhs,
                &revoke_access_accounts_did_pkh,
            );

            let mut operations = Vec::with_capacity(
                blockchain_based_operations.len() + project_based_operations.len(),
            );
            operations.extend(blockchain_based_operations);
            operations.extend(project_based_operations);
            operations
        } else {
            blockchain_based_operations
        };

        Ok(operations)
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn initial_access_applying(&self, app_state: &mut AppState) -> eyre::Result<()> {
        for (ipnft_uid, ipnft_state) in &app_state.ipnft_state_map {
            let operations = self
                .initial_access_applying_for_ipnft(
                    ipnft_uid,
                    ipnft_state,
                    &mut app_state.multisig,
                    app_state.latest_indexed_block_number,
                )
                .await?;

            // Apply operations
            if !operations.is_empty() {
                app_state.access_changes.insert(
                    Utc::now(),
                    AccessChanges {
                        reason: format!(
                            "IPNFT ({:?}/{ipnft_uid}) initial update",
                            ipnft_state.ipnft.symbol
                        ),
                        operations: operations.clone(),
                    },
                );
            }

            self.kamu_node_api_client
                .apply_account_dataset_relations(operations)
                .await?;
        }

        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all, fields(symbol = ipnft_state.ipnft.symbol, ipnft_uid = %ipnft_uid))]
    async fn initial_access_applying_for_ipnft(
        &self,
        ipnft_uid: &IpnftUid,
        ipnft_state: &IpnftState,
        multisig: &mut HashMap<Address, Option<MultisigState>>,
        to_block: u64,
    ) -> eyre::Result<Vec<AccountDatasetRelationOperation>> {
        if ipnft_state.ipnft.burnt {
            tracing::info!("Skip burnt IPNFT");
            return Ok(Vec::new());
        }

        let Some(project) = &ipnft_state.project else {
            tracing::info!("Skip IPNFT since there is no project created for it");
            return Ok(Vec::new());
        };

        // Prepare account information
        let GetAccountsByIpnftStateResponse {
            current_owners,
            holders,
            revoke_access_accounts,
        } = self
            .get_accounts_by_ipnft_state(ipnft_state, multisig, to_block)
            .await?;

        // Create accounts
        let CreateAccountsResponse {
            current_owners_did_pkhs,
            holders_did_pkhs,
            revoke_access_accounts_did_pkh,
        } = self.create_did_pkh_accounts(current_owners, holders, revoke_access_accounts)?;

        let all_accounts_count = current_owners_did_pkhs.len()
            + holders_did_pkhs.len()
            + revoke_access_accounts_did_pkh.len();
        let accounts = {
            let mut v = Vec::with_capacity(all_accounts_count);
            v.extend(current_owners_did_pkhs.clone());
            v.extend(holders_did_pkhs.clone());
            v.extend(revoke_access_accounts_did_pkh.clone());
            v
        };

        self.kamu_node_api_client
            .create_wallet_accounts(accounts)
            .await?;

        // Apply operations
        let project_dataset_ids = get_project_dataset_ids(project);
        let operations = build_operations(
            project_dataset_ids,
            &current_owners_did_pkhs,
            &holders_did_pkhs,
            &revoke_access_accounts_did_pkh,
        );

        Ok(operations)
    }

    #[tracing::instrument(level = "debug", skip_all, fields(address = %address, to_block = %to_block))]
    async fn get_owners(
        &self,
        address: Address,
        multisig: &mut HashMap<Address, Option<MultisigState>>,
        to_block: u64,
    ) -> eyre::Result<GetOwnersResponse> {
        let multisig_state_vacant_entry = match multisig.entry(address) {
            Entry::Occupied(maybe_multisig_occupied_entry) => {
                // Extract information about an already known address:
                let res = maybe_multisig_occupied_entry
                    .get()
                    .as_ref()
                    // 1) If a known multisig wallet
                    .map(|multisig| GetOwnersResponse {
                        current_owners: multisig.current_owners.clone(),
                        former_owners: multisig.former_owners.clone(),
                    })
                    // 2) If a known regular wallet
                    .unwrap_or_else(|| GetOwnersResponse {
                        current_owners: HashSet::from([address]),
                        former_owners: Default::default(),
                    });
                // Early return for readability
                return Ok(res);
            }
            Entry::Vacant(multisig_state_vacant_entry) => multisig_state_vacant_entry,
        };

        // Check if the address belongs to Safe Wallet
        let Some(multisig_owners_from_api) =
            self.multisig_resolver.get_multisig_owners(address).await?
        else {
            // Remember that it's not a multisig account ...
            multisig_state_vacant_entry.insert(None);
            // ... and early return for readability
            return Ok(GetOwnersResponse {
                current_owners: HashSet::from([address]),
                former_owners: Default::default(),
            });
        };

        // From SafeWalletApiService we can only get current owners, but we are also interested in former ones.
        // Restore state up to the requested block (typically the last finalized block).

        // Safe Wallet before v1.3.0 did not have the SafeSetup event that would allow using logs
        // only to restore the full ownership history (https://github.com/safe-global/safe-smart-account/issues/233).
        // Therefore, we use the current owners list from the API and the for former owners from the RemovedOwner event.

        let mut new_multisig_state = MultisigState {
            current_owners: multisig_owners_from_api,
            former_owners: Default::default(),
        };

        self.rpc_client
            .get_logs_ext(
                vec![address],
                HashSet::from_iter([Safe::RemovedOwner::SIGNATURE_HASH]),
                0, // From the beginning
                to_block,
                &mut |logs_chunk| {
                    for log in logs_chunk.logs {
                        match log.event_signature_hash() {
                            Safe::RemovedOwner::SIGNATURE_HASH => {
                                let removed_owner = parse_safe_removed_owner_event(&log.inner)?;

                                if !new_multisig_state.current_owners.contains(&removed_owner) {
                                    new_multisig_state.former_owners.insert(removed_owner);
                                }
                            }
                            unknown_event_signature_hash => {
                                bail!("Unknown Safe event signature hash: {unknown_event_signature_hash}")
                            }
                        }
                    }

                    Ok(())
                },
            )
            .await?;

        let res = GetOwnersResponse {
            current_owners: new_multisig_state.current_owners.clone(),
            former_owners: new_multisig_state.former_owners.clone(),
        };

        // Remember multisig data for subsequent requests.
        multisig_state_vacant_entry.insert(Some(new_multisig_state));

        Ok(res)
    }

    fn create_did_phk(&self, address: Address) -> eyre::Result<DidPhk> {
        DidPhk::new_from_chain_id(self.config.chain_id, address)
    }

    fn create_did_pkh_accounts(
        &self,
        current_owners: HashSet<Address>,
        holders: HashSet<Address>,
        revoke_access_accounts: HashSet<Address>,
    ) -> eyre::Result<CreateAccountsResponse> {
        let mut current_owners_did_pkhs = Vec::with_capacity(current_owners.len());
        for current_owner in current_owners {
            let account = self.create_did_phk(current_owner)?;
            current_owners_did_pkhs.push(account);
        }

        let mut holders_did_pkhs = Vec::with_capacity(holders.len());
        for holder in holders {
            let account = self.create_did_phk(holder)?;
            holders_did_pkhs.push(account);
        }

        let mut revoke_access_accounts_did_pkh = Vec::with_capacity(revoke_access_accounts.len());
        for holder in revoke_access_accounts {
            let account = self.create_did_phk(holder)?;
            revoke_access_accounts_did_pkh.push(account);
        }

        Ok(CreateAccountsResponse {
            current_owners_did_pkhs,
            holders_did_pkhs,
            revoke_access_accounts_did_pkh,
        })
    }

    async fn get_accounts_by_ipnft_state(
        &self,
        ipnft_state: &IpnftState,
        multisig: &mut HashMap<Address, Option<MultisigState>>,
        to_block: u64,
    ) -> eyre::Result<GetAccountsByIpnftStateResponse> {
        let mut current_owners = HashSet::new();
        let mut holders = HashSet::new();
        let mut revoke_access_accounts = HashSet::new();

        // TODO: self.get_owners() in parallel for all possible multisig?
        if let Some(current_owner) = &ipnft_state.ipnft.current_owner {
            let GetOwnersResponse {
                current_owners: new_owners,
                former_owners,
            } = self.get_owners(*current_owner, multisig, to_block).await?;
            current_owners.extend(new_owners);
            revoke_access_accounts.extend(former_owners);
        }
        if let Some(former_owner) = &ipnft_state.ipnft.former_owner {
            let GetOwnersResponse {
                current_owners: old_owners,
                former_owners,
            } = self.get_owners(*former_owner, multisig, to_block).await?;
            revoke_access_accounts.extend(old_owners);
            revoke_access_accounts.extend(former_owners);
        }

        if let Some(token) = &ipnft_state.token {
            for (holder, balance) in &token.holder_balances {
                if *balance > IPT_ACCESS_THRESHOLD {
                    holders.insert(*holder);
                } else {
                    revoke_access_accounts.insert(*holder);
                }
            }
        }

        account_access_sanity_checks(&current_owners, &mut holders, &mut revoke_access_accounts);

        Ok(GetAccountsByIpnftStateResponse {
            current_owners,
            holders,
            revoke_access_accounts,
        })
    }
}

#[derive(Debug)]
struct IndexingResponse {
    ipnft_changes_map: HashMap<IpnftUid, IpnftChanges>,
}

#[derive(Debug, Default)]
struct IpnftChanges {
    minted_and_burnt: bool,
    owner_changes: OwnerChanges,
    holder_balances_changes: HashMap<Address, U256>,
    changed_files: Vec<ChangedVersionedFile>,
}

#[derive(Debug, Default)]
struct OwnerChanges {
    former_owner: Option<Address>,
    current_owner: Option<Address>,
}

struct IndexIpnftAndTokenizerContractsResponse {
    ipnft_events: Vec<IpnftEvent>,
    tokenizer_events: Vec<TokenizerEvent>,
}

#[derive(Debug, Default)]
struct IndexMultisigSafesResponse {
    changed_ipnft_multisig_owners: HashMap<IpnftUid, Address>,
}

struct ProcessTokenizerEventsResponse {
    minimal_ipt_birth_block: u64,
}

struct ProcessTokenTransferEventsResponse {
    participating_holders_balances: HashMap<IpnftUid, HashMap<Address, U256>>,
}

#[derive(Debug)]
struct ChangedVersionedFile {
    dataset_id: DatasetID,
    change: IpnftDataRoomFileChange,
}

type ChangedVersionedFilePerProjectMap = HashMap<IpnftUid, Vec<ChangedVersionedFile>>;

#[derive(Debug)]
enum IpnftDataRoomFileChange {
    Added(MoleculeAccessLevel),
    Removed,
    MoleculeAccessLevelChanged {
        #[expect(dead_code)]
        from: MoleculeAccessLevel,
        to: MoleculeAccessLevel,
    },
}

#[derive(Debug, Default)]
struct GetOwnersResponse {
    current_owners: HashSet<Address>,
    former_owners: HashSet<Address>,
}

struct CreateAccountsResponse {
    current_owners_did_pkhs: Vec<DidPhk>,
    holders_did_pkhs: Vec<DidPhk>,
    revoke_access_accounts_did_pkh: Vec<DidPhk>,
}

#[derive(Debug, Default)]
struct ProjectDatasetIds<'a> {
    core_file_dataset_ids: Vec<&'a DatasetID>,
    owner_file_dataset_ids: Vec<&'a DatasetID>,
    holder_file_dataset_ids: Vec<&'a DatasetID>,
    removed_file_dataset_ids: Vec<&'a DatasetID>,
}

// Helper methods
fn build_added_file_entries_with_molecule_access_level_map(
    added_entities: ChangedVersionedFiles,
    molecule_access_levels_map: &MoleculeAccessLevelEntryMap,
) -> HashMap<DatasetID, VersionedFileEntryWithMoleculeAccessLevel> {
    added_entities
        .into_iter()
        .filter_map(|(dataset_id, file_entry)| {
            let Some(access) = molecule_access_levels_map.get(&dataset_id) else {
                tracing::warn!(
                    "Skip '{}' file ({dataset_id}) because molecule_access_level is missing for it",
                    file_entry.path,
                );

                return None;
            };

            Some((
                dataset_id,
                VersionedFileEntryWithMoleculeAccessLevel {
                    entry: file_entry,
                    molecule_access_level: *access,
                },
            ))
        })
        .collect()
}

struct GetAccountsByIpnftStateResponse {
    current_owners: HashSet<Address>,
    holders: HashSet<Address>,
    revoke_access_accounts: HashSet<Address>,
}

fn prepare_changes_based_on_changed_versioned_files_entries(
    versioned_files_entries: &VersionedFilesEntries,
    molecule_access_levels_map: &MoleculeAccessLevelEntryMap,
) -> Vec<ChangedVersionedFile> {
    let mut changes = Vec::with_capacity(
        versioned_files_entries.added_entities.len()
            + versioned_files_entries.removed_entities.len(),
    );

    for (added_dataset_id, versioned_file_entry) in &versioned_files_entries.added_entities {
        let Some(molecule_access_levels) =
            molecule_access_levels_map.get(added_dataset_id).copied()
        else {
            tracing::warn!(
                "Skip '{}' adding file ({added_dataset_id}) because molecule_access_level is missing for it",
                versioned_file_entry.path,
            );
            continue;
        };

        changes.push(ChangedVersionedFile {
            dataset_id: added_dataset_id.clone(),
            change: IpnftDataRoomFileChange::Added(molecule_access_levels),
        });
    }
    for removed_dataset_id in versioned_files_entries.removed_entities.keys() {
        changes.push(ChangedVersionedFile {
            dataset_id: removed_dataset_id.clone(),
            change: IpnftDataRoomFileChange::Removed,
        });
    }

    changes
}

fn prepare_changes_based_on_changed_molecule_access_levels(
    project_actual_files_map: &HashMap<DatasetID, VersionedFileEntryWithMoleculeAccessLevel>,
    molecule_access_levels_map: &MoleculeAccessLevelEntryMap,
) -> Vec<ChangedVersionedFile> {
    let mut changes = Vec::new();

    for (dataset_id, versioned_file) in project_actual_files_map {
        let current_access = versioned_file.molecule_access_level;
        let Some(new_access) = molecule_access_levels_map.get(dataset_id).copied() else {
            tracing::warn!(
                "Skip '{}' file ({dataset_id}) because molecule_access_level is missing for it",
                versioned_file.entry.path,
            );
            continue;
        };

        if current_access != new_access {
            changes.push(ChangedVersionedFile {
                dataset_id: dataset_id.clone(),
                change: IpnftDataRoomFileChange::MoleculeAccessLevelChanged {
                    from: current_access,
                    to: new_access,
                },
            });
        }
    }

    changes
}

fn account_access_sanity_checks(
    current_owners: &HashSet<Address>,
    holders: &mut HashSet<Address>,
    revoke_access_accounts: &mut HashSet<Address>,
) {
    for owner in current_owners {
        holders.remove(owner);
        revoke_access_accounts.remove(owner);
    }
    for holder in holders.iter() {
        revoke_access_accounts.remove(holder);
    }
}

fn partition_dataset_id_by_molecule_access_level<'a>(
    dataset_id: &'a DatasetID,
    molecule_access_level: MoleculeAccessLevel,
    owner_file_dataset_ids: &mut Vec<&'a DatasetID>,
    holder_file_dataset_ids: &mut Vec<&'a DatasetID>,
) {
    use MoleculeAccessLevel as Access;

    match molecule_access_level {
        Access::Public | Access::Holder => {
            holder_file_dataset_ids.push(dataset_id);
        }
        Access::Admin | Access::Admin2 => {
            owner_file_dataset_ids.push(dataset_id);
        }
    }
}

fn get_project_dataset_ids(project: &ProjectProjection) -> ProjectDatasetIds<'_> {
    let mut owner_file_dataset_ids = Vec::new();
    let mut holder_file_dataset_ids = Vec::new();

    for (dataset_id, entry_with_access_level) in &project.actual_files_map {
        partition_dataset_id_by_molecule_access_level(
            dataset_id,
            entry_with_access_level.molecule_access_level,
            &mut owner_file_dataset_ids,
            &mut holder_file_dataset_ids,
        );
    }

    let mut removed_file_dataset_ids = Vec::new();
    removed_file_dataset_ids.extend(project.removed_files_map.keys());

    ProjectDatasetIds {
        core_file_dataset_ids: vec![
            &project.entry.data_room_dataset_id,
            &project.entry.announcements_dataset_id,
        ],
        owner_file_dataset_ids,
        holder_file_dataset_ids,
        removed_file_dataset_ids,
    }
}

fn build_operations(
    ProjectDatasetIds {
        core_file_dataset_ids,
        owner_file_dataset_ids,
        holder_file_dataset_ids,
        removed_file_dataset_ids,
    }: ProjectDatasetIds,
    current_owners_did_pkhs: &[DidPhk],
    holders_did_pkhs: &[DidPhk],
    revoke_access_accounts_did_pkh: &[DidPhk],
) -> Vec<AccountDatasetRelationOperation> {
    let all_accounts_count = current_owners_did_pkhs.len()
        + holders_did_pkhs.len()
        + revoke_access_accounts_did_pkh.len();
    let all_datasets_count = core_file_dataset_ids.len()
        + owner_file_dataset_ids.len()
        + holder_file_dataset_ids.len()
        + removed_file_dataset_ids.len();

    let mut operations = Vec::with_capacity(all_accounts_count * all_datasets_count);

    for core_file_dataset_id in core_file_dataset_ids {
        for owner in current_owners_did_pkhs {
            operations.push(AccountDatasetRelationOperation::maintainer_access(
                owner.to_string(),
                (*core_file_dataset_id).clone(),
            ));
        }
        for holder in holders_did_pkhs {
            operations.push(AccountDatasetRelationOperation::reader_access(
                holder.to_string(),
                (*core_file_dataset_id).clone(),
            ));
        }
        for revoke_access_account in revoke_access_accounts_did_pkh {
            operations.push(AccountDatasetRelationOperation::revoke_access(
                revoke_access_account.to_string(),
                (*core_file_dataset_id).clone(),
            ));
        }
    }
    for owner_file_dataset_id in owner_file_dataset_ids {
        for owner in current_owners_did_pkhs {
            operations.push(AccountDatasetRelationOperation::maintainer_access(
                owner.to_string(),
                (*owner_file_dataset_id).clone(),
            ));
        }
        for holder in holders_did_pkhs {
            operations.push(AccountDatasetRelationOperation::revoke_access(
                holder.to_string(),
                (*owner_file_dataset_id).clone(),
            ));
        }
        for revoke_access_account in revoke_access_accounts_did_pkh {
            operations.push(AccountDatasetRelationOperation::revoke_access(
                revoke_access_account.to_string(),
                (*owner_file_dataset_id).clone(),
            ));
        }
    }
    for holder_file_dataset_id in holder_file_dataset_ids {
        for owner in current_owners_did_pkhs {
            operations.push(AccountDatasetRelationOperation::maintainer_access(
                owner.to_string(),
                (*holder_file_dataset_id).clone(),
            ));
        }
        for holder in holders_did_pkhs {
            operations.push(AccountDatasetRelationOperation::reader_access(
                holder.to_string(),
                (*holder_file_dataset_id).clone(),
            ));
        }
        for revoke_access_account in revoke_access_accounts_did_pkh {
            operations.push(AccountDatasetRelationOperation::revoke_access(
                revoke_access_account.to_string(),
                (*holder_file_dataset_id).clone(),
            ));
        }
    }
    for removed_file_dataset_id in removed_file_dataset_ids {
        for owner in current_owners_did_pkhs {
            operations.push(AccountDatasetRelationOperation::revoke_access(
                owner.to_string(),
                (*removed_file_dataset_id).clone(),
            ));
        }
        for holder in holders_did_pkhs {
            operations.push(AccountDatasetRelationOperation::revoke_access(
                holder.to_string(),
                (*removed_file_dataset_id).clone(),
            ));
        }
        for revoke_access_account in revoke_access_accounts_did_pkh {
            operations.push(AccountDatasetRelationOperation::revoke_access(
                revoke_access_account.to_string(),
                (*removed_file_dataset_id).clone(),
            ));
        }
    }

    operations
}

fn parse_safe_added_owner_event(log: &Log) -> eyre::Result<Address> {
    // NOTE: We can use the actual event signature hashes because
    //       the indexed mark doesn't participate in hash calculation.

    // First, try to parse the actual event signature (indexed "owner" field), ...
    let added_owner = if let Ok(event) = Safe::AddedOwner::decode_log(log) {
        event.owner
    } else {
        // Try to parse an old version event (w/o indexed mark) -- may be relevant for older Safe Wallet versions
        let event = safe::v1_3_0::Safe::AddedOwner::decode_log(log)?;
        event.owner
    };

    Ok(added_owner)
}

fn parse_safe_removed_owner_event(log: &Log) -> eyre::Result<Address> {
    // First, try to parse the actual event signature (indexed "owner" field), ...
    let removed_owner = if let Ok(event) = Safe::RemovedOwner::decode_log(log) {
        event.owner
    } else {
        // Try to parse an old version event (w/o indexed mark) -- may be relevant for older Safe Wallet versions
        let event = safe::v1_3_0::Safe::RemovedOwner::decode_log(log)?;
        event.owner
    };

    Ok(removed_owner)
}
