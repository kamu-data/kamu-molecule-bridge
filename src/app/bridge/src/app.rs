use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::primitives::{Address, Log};
use alloy::providers::DynProvider;
use alloy_ext::prelude::*;
use chrono::{DateTime, Utc};
use color_eyre::eyre;
use color_eyre::eyre::bail;
use kamu_node_api_client::*;
use molecule_contracts::prelude::*;
use molecule_contracts::{LabNFT, Safe, safe};
use molecule_ocl::entities::{
    OclId, OclOwnershipChange, OclOwnershipProjection, OclOwnershipProjectionMap, OclTransferEvent,
};
use multisig::services::MultisigResolver;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::Instrument as _;

use crate::config::Config;
use crate::http_server;
use crate::http_server::{HttpServeFuture, StateRequester};
use crate::metrics::BridgeMetrics;

// TODO: Implement event sourcing: maintain single ordered log of events from two sources
//       and derive state as a projection over that log.
//
//       Sources:
//       - On-chain: LabNFT transfers, multisig changes (source of truth, indexed by block number)
//       - Off-chain: OCL project changes (polled periodically, indexed by offset)
//
//       Each event should carry a timestamp for cross-source ordering.

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
pub struct AppState {
    molecule_projects_dataset_offset: Option<u64>,
    molecule_projects_last_requested_at: Option<DateTime<Utc>>,

    on_chain_ocl_ownership_projection_map: OclOwnershipProjectionMap,
    off_chain_ocl_project_map: HashMap<OclId, OffChainMoleculeProjectProjection>,
    latest_indexed_block_number: u64,

    multisig: HashMap<Address, Option<MultisigState>>,

    access_changes: HashMap<DateTime<Utc>, AccessChanges>,
}

#[derive(Debug, Serialize)]
struct AccessChanges {
    reason: String,
    operations: Vec<AccountDatasetRelationOperation>,
}

#[async_trait::async_trait]
impl StateRequester for RwLock<AppState> {
    async fn request_as_json(&self) -> Value {
        let readable_state = self.read().await;
        serde_json::to_value(&*readable_state).unwrap()
    }
}

#[derive(Debug, Serialize)]
struct MultisigState {
    current_owners: HashSet<Address>,
    former_owners: HashSet<Address>,
}

#[derive(Debug, Serialize)]
struct OffChainMoleculeProjectProjection {
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

        let labnft_contract_birth_block_minus_one = self.config.labnft_contract_birth_block - 1;

        let mut initial_app_state = AppState {
            latest_indexed_block_number: labnft_contract_birth_block_minus_one,
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
            // NOTE: emphasize that this includes not just on-chain changes
            on_chain_ocl_changes_map: mut ocl_changes_map,
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

            for (ocl_id, changed_files) in versioned_file_changes_per_projects {
                let ocl_changes = ocl_changes_map.entry(ocl_id).or_default();
                // TODO: do not reuse changed_files field -- add a new struct
                ocl_changes.changed_files = changed_files;
            }

            writable_state.molecule_projects_last_requested_at = Some(Utc::now());
        }

        self.interval_access_applying(
            &mut writable_state,
            ocl_changes_map,
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
        // TODO parallel jobs index_labnft_contract && changed_ocl_multisig_owners

        let ocl_transfer_events = self
            .index_labnft_contract(app_state.latest_indexed_block_number + 1, to_block)
            .await?;
        let ocl_ownership_diff_map = app_state
            .on_chain_ocl_ownership_projection_map
            .apply_events(ocl_transfer_events);

        // TODO breakdown to unblock parallel calls
        let IndexMultisigSafesResponse {
            changed_ocl_multisig_owners,
        } = self
            .index_multisig_safes(
                app_state,
                app_state.latest_indexed_block_number + 1,
                to_block,
            )
            .await?;

        app_state.latest_indexed_block_number = to_block;

        // Populate blockchain changes:

        // 1. From LabNFT contract
        let mut ocl_changes_map: HashMap<_, _> = ocl_ownership_diff_map
            .into_iter()
            .map(|(ocl_id, ownership_change)| (ocl_id, OclChange::new(ownership_change)))
            .collect();

        // 2. From multisig changes
        for (ocl_id, owner) in changed_ocl_multisig_owners {
            let ocl_change = ocl_changes_map.entry(ocl_id).or_default();

            // If the current owner changes, we will request new data from the multisig state if needed.
            if ocl_change.owner_changes.is_none() {
                // If there is no owner change, we need to trigger new permissions [re]grant in an OCL.
                ocl_change.owner_changes = Some(OclOwnershipChange {
                    former_owner: None,
                    current_owner: owner,
                });
            }
        }

        Ok(IndexingResponse {
            on_chain_ocl_changes_map: ocl_changes_map,
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
    async fn index_labnft_contract(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> eyre::Result<Vec<OclTransferEvent>> {
        // TODO: static/const
        let event_signatures = HashSet::from_iter([LabNFT::OclTransfer::SIGNATURE_HASH]);

        let mut events = Vec::new();

        self.rpc_client
            .get_logs_ext(
                // TODO: add method for only one address?
                vec![self.config.labnft_contract_address],
                event_signatures,
                from_block,
                to_block,
                &mut |logs_chunk| {
                    for log in logs_chunk.logs {
                        match log.event_signature_hash() {
                            LabNFT::OclTransfer::SIGNATURE_HASH => {
                                let log_event = LabNFT::OclTransfer::decode_log(&log.inner)?;
                                let event = log_event.data;

                                events.push(event.into());
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
                // TODO: static/const
                HashSet::from_iter([
                    Safe::AddedOwner::SIGNATURE_HASH,
                    Safe::RemovedOwner::SIGNATURE_HASH,
                ]),
                from_block,
                to_block,
                &mut |logs_chunk| {
                    for log in logs_chunk.logs {
                        let safe_address = log.address();

                        // TODO: move state from here
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

        // TODO breakdown to unblock parallel calls
        let changed_ocl_multisig_owners = app_state
            .on_chain_ocl_ownership_projection_map
            .iter()
            .fold(HashMap::new(), |mut acc, (ocl_id, ownership_projection)| {
                if let Some(owner) = ownership_projection.current
                    && changed_multisigs.contains(&owner)
                {
                    acc.insert(*ocl_id, owner);
                }
                acc
            });

        Ok(IndexMultisigSafesResponse {
            changed_ocl_multisig_owners,
        })
    }

    // TODO: breakdown to smaller peaces
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
            .off_chain_ocl_project_map
            .values_mut()
            .collect::<Vec<_>>();
        let existing_data_room_dataset_ids_with_offsets = existing_projects
            .iter()
            .map(|project| DataRoomDatasetIdWithOffset {
                dataset_id: project.entry.data_room_dataset_id.clone(),
                offset: project.latest_data_room_offset + 1,
            })
            .collect::<Vec<_>>();

        // Second, check for new project entries.
        let new_projects_entries = self
            .kamu_node_api_client
            .get_molecule_project_entries(
                app_state
                    .molecule_projects_dataset_offset
                    .map(|offset| offset + 1)
                    .unwrap_or(0),
                self.config.ignore_ocl_ids.as_ref(),
            )
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
                ocl_id = %project_entry.ocl_id
            )
            .entered();

            let Some(versioned_files_entries) = versioned_files_entries_map
                // NOTE: try to extract a value from the map
                .remove(&project_entry.data_room_dataset_id)
            else {
                continue;
            };

            let changed_versioned_files = prepare_changes_based_on_changed_versioned_files_entries(
                project_entry,
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
                project_entry,
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
                detected_changes_map.insert(project_entry.ocl_id, detected_changes);
            }
        }

        // III. Process new project entries.
        // NOTE: Projects are sorted, so we can simply assign each new value.
        let mut new_molecule_projects_dataset_offset = app_state.molecule_projects_dataset_offset;

        for project_entry in new_projects_entries {
            let mut detected_changes = Vec::new();

            let _span = tracing::debug_span!(
                "Process new project entry",
                symbol = project_entry.symbol,
                ocl_id = %project_entry.ocl_id
            )
            .entered();

            new_molecule_projects_dataset_offset = Some(project_entry.offset);

            if app_state
                .on_chain_ocl_ownership_projection_map
                .get(&project_entry.ocl_id)
                .is_none()
            {
                tracing::info!("Skip project: not present in blockchain");
                continue;
            }

            let Some(versioned_files_entries) = versioned_files_entries_map
                // NOTE: try to extract a value from the map
                .remove(&project_entry.data_room_dataset_id)
            else {
                tracing::info!("Skip project: data-room is empty");
                continue;
            };

            tracing::debug!(
                "Continue: project is present in blockchain and has files in data-room"
            );

            let changed_versioned_files = prepare_changes_based_on_changed_versioned_files_entries(
                &project_entry,
                &versioned_files_entries,
                &molecule_access_levels_map,
            );
            detected_changes.extend(changed_versioned_files);

            let actual_files_map = build_added_file_entries_with_molecule_access_level_map(
                versioned_files_entries.added_entities,
                &molecule_access_levels_map,
            );

            if !detected_changes.is_empty() {
                detected_changes_map.insert(project_entry.ocl_id, detected_changes);
            }

            app_state.off_chain_ocl_project_map.insert(
                project_entry.ocl_id,
                OffChainMoleculeProjectProjection {
                    entry: project_entry,
                    latest_data_room_offset: versioned_files_entries.latest_data_room_offset,
                    actual_files_map,
                    removed_files_map: versioned_files_entries.removed_entities,
                },
            );
        }

        app_state.molecule_projects_dataset_offset = new_molecule_projects_dataset_offset;

        Ok(detected_changes_map)
    }

    #[tracing::instrument(
        level = "info",
        skip_all,
        fields(
            changed_ocls_count = ocl_changes_map.len(),
        )
    )]
    async fn interval_access_applying(
        &self,
        app_state: &mut AppState,
        ocl_changes_map: HashMap<OclId, OclChange>,
        to_block: u64,
    ) -> eyre::Result<()> {
        for (ocl_id, ocl_change) in ocl_changes_map {
            tracing::info!(%ocl_id, "OCL interval update");

            let Some(on_chain_ocl_ownership) =
                app_state.on_chain_ocl_ownership_projection_map.get(&ocl_id)
            else {
                tracing::info!("Skip OCL update: not found on-chain");
                assert!(ocl_change.owner_changes.is_none());
                continue;
            };

            let Some(off_chain_ocl_project) = app_state.off_chain_ocl_project_map.get(&ocl_id)
            else {
                tracing::info!("Skip OCL update: no entry in \"projects\" dataset");
                assert!(ocl_change.changed_files.is_empty());
                continue;
            };

            let operations = self
                .interval_access_applying_for_ocl(
                    ocl_id,
                    on_chain_ocl_ownership,
                    off_chain_ocl_project,
                    ocl_change,
                    &mut app_state.multisig,
                    to_block,
                )
                .await?;

            // Apply operations
            if !operations.is_empty() {
                let symbol = &off_chain_ocl_project.entry.symbol;

                app_state.access_changes.insert(
                    Utc::now(),
                    AccessChanges {
                        reason: format!("OCL ({ocl_id}/{symbol}) interval update"),
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

    // TODO: breakdown to smaller peaces
    // TODO: is symbol needed like before?
    #[tracing::instrument(level = "info", skip_all, fields(ocl_id = %ocl_id))]
    async fn interval_access_applying_for_ocl(
        &self,
        ocl_id: OclId,
        on_chain_ocl_ownership: &OclOwnershipProjection,
        off_chain_ocl_project: &OffChainMoleculeProjectProjection,
        ocl_change: OclChange,
        multisig: &mut HashMap<Address, Option<MultisigState>>,
        to_block: u64,
    ) -> eyre::Result<Vec<AccountDatasetRelationOperation>> {
        // 1. Process new blockchain data.
        let blockchain_based_operations = {
            // Prepare account information
            let mut current_owners = HashSet::new();
            let mut revoke_access_accounts = HashSet::new();

            // TODO: PERF: self.get_owners() in parallel for all possible multisig?
            if let Some(owner_changes) = ocl_change.owner_changes {
                let GetOwnersResponse {
                    current_owners: new_owners,
                    former_owners,
                } = self
                    .get_owners(owner_changes.current_owner, multisig, to_block)
                    .await?;
                current_owners.extend(new_owners);
                revoke_access_accounts.extend(former_owners);

                if let Some(former_owner) = owner_changes.former_owner {
                    let GetOwnersResponse {
                        current_owners: old_owners,
                        former_owners,
                    } = self.get_owners(former_owner, multisig, to_block).await?;
                    revoke_access_accounts.extend(old_owners);
                    revoke_access_accounts.extend(former_owners);
                }
            }

            account_access_sanity_checks(&current_owners, &mut revoke_access_accounts);

            // Create accounts
            let CreateAccountsResponse {
                current_owners_did_pkhs,
                revoke_access_accounts_did_pkh,
            } = self.create_did_pkh_accounts(current_owners, revoke_access_accounts)?;

            let all_accounts_count =
                current_owners_did_pkhs.len() + revoke_access_accounts_did_pkh.len();
            let accounts = {
                let mut v = Vec::with_capacity(all_accounts_count);
                v.extend(current_owners_did_pkhs.clone());
                v.extend(revoke_access_accounts_did_pkh.clone());
                v
            };

            self.kamu_node_api_client
                .create_wallet_accounts(accounts)
                .await?;

            let project_dataset_ids = get_project_dataset_ids(off_chain_ocl_project);

            build_operations(
                project_dataset_ids,
                &current_owners_did_pkhs,
                &revoke_access_accounts_did_pkh,
            )
        };

        // 2. Process the project's changes.
        let operations = if !ocl_change.changed_files.is_empty() {
            let GetAccountsByOclProjectResponse {
                current_owners,
                revoke_access_accounts,
            } = self
                .get_accounts_by_ocl_project(on_chain_ocl_ownership, multisig, to_block)
                .await?;
            let CreateAccountsResponse {
                current_owners_did_pkhs,
                revoke_access_accounts_did_pkh,
            } = self.create_did_pkh_accounts(current_owners, revoke_access_accounts)?;

            let mut changed_project_dataset_ids = ProjectDatasetIds::default();

            for changed_file in &ocl_change.changed_files {
                match changed_file.change {
                    DataRoomFileChange::Added(molecule_access_level) => {
                        partition_dataset_id_by_molecule_access_level(
                            &changed_file.dataset_id,
                            molecule_access_level,
                            &mut changed_project_dataset_ids.owner_file_dataset_ids,
                            &mut changed_project_dataset_ids.holder_file_dataset_ids,
                        );
                    }
                    DataRoomFileChange::Removed => {
                        changed_project_dataset_ids
                            .removed_file_dataset_ids
                            .push(&changed_file.dataset_id);
                    }
                    DataRoomFileChange::MoleculeAccessLevelChanged { from: _, to } => {
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
        for (ocl_id, off_chain_ocl_project) in &app_state.off_chain_ocl_project_map {
            let symbol = &off_chain_ocl_project.entry.symbol;

            tracing::info!(%ocl_id, symbol, "OCL initial update");

            let Some(on_chain_ocl_ownership) =
                app_state.on_chain_ocl_ownership_projection_map.get(ocl_id)
            else {
                tracing::info!("Skip OCL update: not found on-chain");
                continue;
            };

            let operations = self
                .initial_access_applying_for_ocl(
                    *ocl_id,
                    on_chain_ocl_ownership,
                    off_chain_ocl_project,
                    &mut app_state.multisig,
                    app_state.latest_indexed_block_number,
                )
                .await?;

            // Apply operations
            if !operations.is_empty() {
                app_state.access_changes.insert(
                    Utc::now(),
                    AccessChanges {
                        reason: format!("OCL ({ocl_id}/{symbol}) initial update"),
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

    // TODO: is symbol needed like before?
    #[tracing::instrument(level = "info", skip_all, fields(ocl_id = %ocl_id))]
    async fn initial_access_applying_for_ocl(
        &self,
        ocl_id: OclId,
        on_chain_ocl_ownership: &OclOwnershipProjection,
        off_chain_ocl_project: &OffChainMoleculeProjectProjection,
        multisig: &mut HashMap<Address, Option<MultisigState>>,
        to_block: u64,
    ) -> eyre::Result<Vec<AccountDatasetRelationOperation>> {
        // Prepare account information
        let GetAccountsByOclProjectResponse {
            current_owners,
            revoke_access_accounts,
        } = self
            .get_accounts_by_ocl_project(on_chain_ocl_ownership, multisig, to_block)
            .await?;

        // Create accounts
        let CreateAccountsResponse {
            current_owners_did_pkhs,
            revoke_access_accounts_did_pkh,
        } = self.create_did_pkh_accounts(current_owners, revoke_access_accounts)?;

        let all_accounts_count =
            current_owners_did_pkhs.len() + revoke_access_accounts_did_pkh.len();
        let accounts = {
            let mut v = Vec::with_capacity(all_accounts_count);
            v.extend(current_owners_did_pkhs.clone());
            v.extend(revoke_access_accounts_did_pkh.clone());
            v
        };

        self.kamu_node_api_client
            .create_wallet_accounts(accounts)
            .await?;

        // Apply operations
        let project_dataset_ids = get_project_dataset_ids(off_chain_ocl_project);
        let operations = build_operations(
            project_dataset_ids,
            &current_owners_did_pkhs,
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
        revoke_access_accounts: HashSet<Address>,
    ) -> eyre::Result<CreateAccountsResponse> {
        let mut current_owners_did_pkhs = Vec::with_capacity(current_owners.len());
        for current_owner in current_owners {
            let account = self.create_did_phk(current_owner)?;
            current_owners_did_pkhs.push(account);
        }

        let mut revoke_access_accounts_did_pkh = Vec::with_capacity(revoke_access_accounts.len());
        for holder in revoke_access_accounts {
            let account = self.create_did_phk(holder)?;
            revoke_access_accounts_did_pkh.push(account);
        }

        Ok(CreateAccountsResponse {
            current_owners_did_pkhs,
            revoke_access_accounts_did_pkh,
        })
    }

    async fn get_accounts_by_ocl_project(
        &self,
        on_chain_ocl_ownership: &OclOwnershipProjection,
        multisig: &mut HashMap<Address, Option<MultisigState>>,
        to_block: u64,
    ) -> eyre::Result<GetAccountsByOclProjectResponse> {
        let mut current_owners = HashSet::new();
        let mut revoke_access_accounts = HashSet::new();

        // TODO: PERF: self.get_owners() in parallel for all possible multisig?
        if let Some(current_owner) = &on_chain_ocl_ownership.current {
            let GetOwnersResponse {
                current_owners: new_owners,
                former_owners,
            } = self.get_owners(*current_owner, multisig, to_block).await?;
            current_owners.extend(new_owners);
            revoke_access_accounts.extend(former_owners);
        }

        for previous in &on_chain_ocl_ownership.previous {
            let GetOwnersResponse {
                current_owners: former_owners_1,
                former_owners: former_owners_2,
            } = self.get_owners(*previous, multisig, to_block).await?;
            revoke_access_accounts.extend(former_owners_1);
            revoke_access_accounts.extend(former_owners_2);
        }

        account_access_sanity_checks(&current_owners, &mut revoke_access_accounts);

        Ok(GetAccountsByOclProjectResponse {
            current_owners,
            revoke_access_accounts,
        })
    }
}

#[derive(Debug)]
struct IndexingResponse {
    on_chain_ocl_changes_map: HashMap<OclId, OclChange>,
}

#[derive(Debug, Default)]
struct OclChange {
    owner_changes: Option<OclOwnershipChange>,
    changed_files: Vec<ChangedVersionedFile>,
}

impl OclChange {
    // todo remove?
    fn new(owner_changes: OclOwnershipChange) -> Self {
        Self {
            owner_changes: Some(owner_changes),
            changed_files: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
struct IndexMultisigSafesResponse {
    changed_ocl_multisig_owners:
        HashMap<OclId, /* updated multisig (changed owners) */ Address>,
}

#[derive(Debug)]
struct ChangedVersionedFile {
    dataset_id: DatasetID,
    change: DataRoomFileChange,
}

type ChangedVersionedFilePerProjectMap = HashMap<OclId, Vec<ChangedVersionedFile>>;

#[derive(Debug)]
enum DataRoomFileChange {
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

struct GetAccountsByOclProjectResponse {
    current_owners: HashSet<Address>,
    revoke_access_accounts: HashSet<Address>,
}

fn prepare_changes_based_on_changed_versioned_files_entries(
    project_entry: &MoleculeProjectEntry,
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

        // NOTE: If the project is deleted, consider all files deleted as well.
        let change = if project_entry.is_deleted() {
            DataRoomFileChange::Removed
        } else {
            DataRoomFileChange::Added(molecule_access_levels)
        };

        changes.push(ChangedVersionedFile {
            dataset_id: added_dataset_id.clone(),
            change,
        });
    }
    for removed_dataset_id in versioned_files_entries.removed_entities.keys() {
        changes.push(ChangedVersionedFile {
            dataset_id: removed_dataset_id.clone(),
            change: DataRoomFileChange::Removed,
        });
    }

    changes
}

fn prepare_changes_based_on_changed_molecule_access_levels(
    project_entry: &MoleculeProjectEntry,
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

        // NOTE: If the project is deleted, consider all files deleted as well.
        if project_entry.is_deleted() {
            changes.push(ChangedVersionedFile {
                dataset_id: dataset_id.clone(),
                change: DataRoomFileChange::Removed,
            });
        } else if current_access != new_access {
            changes.push(ChangedVersionedFile {
                dataset_id: dataset_id.clone(),
                change: DataRoomFileChange::MoleculeAccessLevelChanged {
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
    revoke_access_accounts: &mut HashSet<Address>,
) {
    for owner in current_owners {
        revoke_access_accounts.remove(owner);
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

fn get_project_dataset_ids(
    off_chain_project: &OffChainMoleculeProjectProjection,
) -> ProjectDatasetIds<'_> {
    let mut owner_file_dataset_ids = Vec::new();
    let mut holder_file_dataset_ids = Vec::new();

    for (dataset_id, entry_with_access_level) in &off_chain_project.actual_files_map {
        partition_dataset_id_by_molecule_access_level(
            dataset_id,
            entry_with_access_level.molecule_access_level,
            &mut owner_file_dataset_ids,
            &mut holder_file_dataset_ids,
        );
    }

    let mut removed_file_dataset_ids = Vec::new();
    removed_file_dataset_ids.extend(off_chain_project.removed_files_map.keys());

    ProjectDatasetIds {
        core_file_dataset_ids: vec![
            &off_chain_project.entry.data_room_dataset_id,
            &off_chain_project.entry.announcements_dataset_id,
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
    revoke_access_accounts_did_pkh: &[DidPhk],
) -> Vec<AccountDatasetRelationOperation> {
    let all_accounts_count = current_owners_did_pkhs.len() + revoke_access_accounts_did_pkh.len();
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

// TODO: move to molecule-contracts crate
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
