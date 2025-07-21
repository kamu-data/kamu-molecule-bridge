use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::primitives::{Address, U256};
use alloy::providers::DynProvider;
use alloy_ext::prelude::*;
use async_trait::async_trait;
use color_eyre::eyre;
use color_eyre::eyre::{ContextCompat, bail};
use kamu_node_api_client::*;
use molecule_contracts::prelude::*;
use molecule_contracts::{IPNFT, IPToken, Synthesizer, Tokenizer};
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

pub struct App {
    config: Config,
    rpc_client: DynProvider,
    multisig_resolver: Arc<dyn MultisigResolver>,
    kamu_node_api_client: Arc<dyn KamuNodeApiClient>,

    #[expect(dead_code)]
    metrics: BridgeMetrics,
    metrics_registry: prometheus::Registry,

    state: Arc<RwLock<AppState>>,
}

#[derive(Debug, Default, Serialize)]
struct AppState {
    projects_dataset_offset: u64,

    ipnft_state_map: HashMap<IpnftUid, IpnftState>,
    ipnft_latest_indexed_block_number: u64,

    token_address_ipnft_uid_mapping: HashMap<Address, IpnftUid>,
    tokens_latest_indexed_block_number: u64,
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
struct ProjectProjection {
    entry: MoleculeProjectEntry,
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
        Self {
            config,
            rpc_client,
            multisig_resolver,
            kamu_node_api_client,
            metrics,
            metrics_registry,
            state: Default::default(),
        }
    }

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
        let latest_finalized_block_number = self.rpc_client.latest_finalized_block_number().await?;

        let mut initial_app_state = AppState::default();

        self.initial_indexing(&mut initial_app_state, latest_finalized_block_number)
            .await?;
        self.initial_projects_loading(&mut initial_app_state)
            .await?;
        // TODO: index multisig for whitelisted IPNFT
        self.initial_access_applying(&initial_app_state).await?;

        {
            let mut writable_state = self.state.write().await;
            *writable_state = initial_app_state;
        }

        Ok(())
    }

    #[expect(clippy::unused_async)]
    async fn update(&mut self) -> eyre::Result<()> {
        tracing::info!("Performing update loop iteration");
        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all, fields(to_block = to_block))]
    async fn initial_indexing(
        &mut self,
        app_state: &mut AppState,
        to_block: u64,
    ) -> eyre::Result<()> {
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
            app_state.ipnft_state_map.insert(
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
        } = self.process_tokenizer_events(app_state, tokenizer_events);

        app_state.ipnft_latest_indexed_block_number = to_block;

        let token_transfer_events = self
            .index_tokens(app_state, minimal_ipt_birth_block, to_block)
            .await?;
        self.process_token_transfer_events(app_state, token_transfer_events)?;

        app_state.tokens_latest_indexed_block_number = to_block;

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
            diff = to_block - from_block,
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
    ) -> eyre::Result<()> {
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

    #[tracing::instrument(level = "info", skip_all)]
    async fn initial_projects_loading(&mut self, app_state: &mut AppState) -> eyre::Result<()> {
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
                    acc
                });
        let molecule_access_levels_map = self
            .kamu_node_api_client
            .get_latest_molecule_access_levels_by_dataset_ids(versioned_file_dataset_ids)
            .await?;

        let mut projects_dataset_offset = 0;
        for project_entry in all_projects_entries {
            let _span = tracing::debug_span!(
                "Process project",
                symbol = project_entry.symbol,
                ipnft_uid = %project_entry.ipnft_uid
            )
            .entered();

            projects_dataset_offset = project_entry.offset;

            let Some(ipnft_state) = app_state.ipnft_state_map.get_mut(&project_entry.ipnft_uid)
            else {
                tracing::warn!("Skip project because it's not present in blockchain");
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
                .collect();

            ipnft_state.project = Some(ProjectProjection {
                entry: project_entry,
                actual_files_map,
                removed_files_map: versioned_files_entries.removed_entities,
            });
        }

        app_state.projects_dataset_offset = projects_dataset_offset;

        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all)]
    async fn initial_access_applying(&self, app_state: &AppState) -> eyre::Result<()> {
        for ipnft_state_pair in &app_state.ipnft_state_map {
            self.initial_access_applying_for_ipnft(ipnft_state_pair)
                .await?;
        }

        Ok(())
    }

    #[tracing::instrument(level = "info", skip_all, fields(symbol = ipnft_state.ipnft.symbol, ipnft_uid = %ipnft_uid))]
    async fn initial_access_applying_for_ipnft(
        &self,
        (ipnft_uid, ipnft_state): (&IpnftUid, &IpnftState),
    ) -> eyre::Result<()> {
        // TODO: Update when it's agreed
        const IPT_ACCESS_THRESHOLD: U256 = U256::ZERO;

        if ipnft_state.ipnft.burnt {
            tracing::info!("Skip burnt IPNFT");
            return Ok(());
        }

        let Some(project) = &ipnft_state.project else {
            tracing::info!("Skip IPNFT since there is no project created for it");
            return Ok(());
        };

        // Prepare file dataset ids
        let mut core_file_dataset_ids = Vec::with_capacity(2);
        let mut owner_file_dataset_ids = Vec::new();
        let mut holder_file_dataset_ids = Vec::new();
        let mut removed_file_dataset_ids = Vec::new();

        core_file_dataset_ids.push(project.entry.data_room_dataset_id.clone());
        core_file_dataset_ids.push(project.entry.announcements_dataset_id.clone());

        for (dataset_id, entry_with_access_level) in &project.actual_files_map {
            use MoleculeAccessLevel as Access;

            match entry_with_access_level.molecule_access_level {
                Access::Public | Access::Holder => {
                    holder_file_dataset_ids.push(dataset_id.clone());
                }
                Access::Admin | Access::Admin2 => {
                    owner_file_dataset_ids.push(dataset_id.clone());
                }
            }
        }

        removed_file_dataset_ids.extend(project.removed_files_map.keys());

        // Prepare account information
        let mut current_owners = HashSet::new();
        let mut holders = HashSet::new();
        let mut revoke_access_accounts = HashSet::new();

        // TODO: self.get_owners() in parallel for all possible multisig?
        if let Some(current_owner) = &ipnft_state.ipnft.current_owner {
            let (owners, _) = self.get_owners(*current_owner).await?;
            current_owners.extend(owners);
        }
        if let Some(former_owner) = &ipnft_state.ipnft.former_owner {
            let (owners, _) = self.get_owners(*former_owner).await?;
            revoke_access_accounts.extend(owners);
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

        // Sanity checks
        for owner in &current_owners {
            holders.remove(owner);
            revoke_access_accounts.remove(owner);
        }
        for holder in &holders {
            revoke_access_accounts.remove(holder);
        }

        // Create accounts
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
        let all_datasets_count = core_file_dataset_ids.len()
            + owner_file_dataset_ids.len()
            + holder_file_dataset_ids.len()
            + removed_file_dataset_ids.len();

        let mut operations = Vec::with_capacity(all_accounts_count * all_datasets_count);

        for core_file_dataset_id in core_file_dataset_ids {
            for owner in &current_owners_did_pkhs {
                operations.push(AccountDatasetRelationOperation::maintainer_access(
                    owner.to_string(),
                    core_file_dataset_id.clone(),
                ));
            }
            for holder in &holders_did_pkhs {
                operations.push(AccountDatasetRelationOperation::reader_access(
                    holder.to_string(),
                    core_file_dataset_id.clone(),
                ));
            }
            for revoke_access_account in &revoke_access_accounts_did_pkh {
                operations.push(AccountDatasetRelationOperation::revoke_access(
                    revoke_access_account.to_string(),
                    core_file_dataset_id.clone(),
                ));
            }
        }
        for owner_file_dataset_id in owner_file_dataset_ids {
            for owner in &current_owners_did_pkhs {
                operations.push(AccountDatasetRelationOperation::maintainer_access(
                    owner.to_string(),
                    owner_file_dataset_id.clone(),
                ));
            }
            for holder in &holders_did_pkhs {
                operations.push(AccountDatasetRelationOperation::revoke_access(
                    holder.to_string(),
                    owner_file_dataset_id.clone(),
                ));
            }
            for revoke_access_account in &revoke_access_accounts_did_pkh {
                operations.push(AccountDatasetRelationOperation::revoke_access(
                    revoke_access_account.to_string(),
                    owner_file_dataset_id.clone(),
                ));
            }
        }
        for holder_file_dataset_id in holder_file_dataset_ids {
            for owner in &current_owners_did_pkhs {
                operations.push(AccountDatasetRelationOperation::maintainer_access(
                    owner.to_string(),
                    holder_file_dataset_id.clone(),
                ));
            }
            for holder in &holders_did_pkhs {
                operations.push(AccountDatasetRelationOperation::reader_access(
                    holder.to_string(),
                    holder_file_dataset_id.clone(),
                ));
            }
            for revoke_access_account in &revoke_access_accounts_did_pkh {
                operations.push(AccountDatasetRelationOperation::revoke_access(
                    revoke_access_account.to_string(),
                    holder_file_dataset_id.clone(),
                ));
            }
        }
        for removed_file_dataset_id in removed_file_dataset_ids {
            for owner in &current_owners_did_pkhs {
                operations.push(AccountDatasetRelationOperation::revoke_access(
                    owner.to_string(),
                    removed_file_dataset_id.clone(),
                ));
            }
            for holder in &holders_did_pkhs {
                operations.push(AccountDatasetRelationOperation::revoke_access(
                    holder.to_string(),
                    removed_file_dataset_id.clone(),
                ));
            }
            for revoke_access_account in &revoke_access_accounts_did_pkh {
                operations.push(AccountDatasetRelationOperation::revoke_access(
                    revoke_access_account.to_string(),
                    removed_file_dataset_id.clone(),
                ));
            }
        }

        self.kamu_node_api_client
            .apply_account_dataset_relations(operations)
            .await?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, fields(address = %address))]
    async fn get_owners(&self, address: Address) -> eyre::Result<(HashSet<Address>, bool)> {
        // TODO: revoke access from former multisig owners

        let maybe_owners = self.multisig_resolver.get_multisig_owners(address).await?;
        let multisig = maybe_owners.is_some();
        let owners = maybe_owners.unwrap_or_else(|| HashSet::from([address]));

        Ok((owners, multisig))
    }

    fn create_did_phk(&self, address: Address) -> eyre::Result<DidPhk> {
        DidPhk::new_from_chain_id(self.config.chain_id, address)
    }
}

struct IndexIpnftAndTokenizerContractsResponse {
    ipnft_events: Vec<IpnftEvent>,
    tokenizer_events: Vec<TokenizerEvent>,
}

struct ProcessTokenizerEventsResponse {
    minimal_ipt_birth_block: u64,
}
