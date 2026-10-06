use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::Arc;

use async_trait::async_trait;
use eyre::bail;
use graphql_client::{GraphQLQuery, Response};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::did_phk::DidPhk;
use crate::*;

const MAX_SQL_QUERY_LIMIT: usize = 10_000;

pub struct KamuNodeApiClientImpl {
    gql_api_endpoint: String,
    token: String,
    molecule_projects_dataset_alias: String,
    http_client: reqwest_middleware::ClientWithMiddleware,

    metric_gql_requests_num_total: prometheus::IntCounter,
    metric_gql_errors_num_total: prometheus::IntCounter,

    data_room_batch_size: NonZeroUsize,
    max_concurrent_data_room_batches: usize,

    dry_run: bool,
}

impl KamuNodeApiClientImpl {
    pub fn new(
        endpoint: String,
        token: String,
        molecule_projects_dataset_alias: String,
        metric_gql_requests_num_total: prometheus::IntCounter,
        metric_gql_errors_num_total: prometheus::IntCounter,
        data_room_batch_size: usize,
        max_concurrent_data_room_batches: usize,
        dry_run: bool,
    ) -> Self {
        let http_client = {
            use reqwest_middleware::ClientBuilder;
            use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

            let retry_policy = ExponentialBackoff::builder().build_with_max_retries(5);
            ClientBuilder::new(reqwest::Client::new())
                .with(RetryTransientMiddleware::new_with_policy(retry_policy))
                .build()
        };

        Self {
            gql_api_endpoint: endpoint,
            token,
            http_client,
            molecule_projects_dataset_alias,
            metric_gql_requests_num_total,
            metric_gql_errors_num_total,
            data_room_batch_size: NonZeroUsize::new(data_room_batch_size)
                .expect("data_room_batch_size must be non-zero"),
            max_concurrent_data_room_batches,
            dry_run,
        }
    }

    async fn get_current_head(&self, dataset_ref: &DatasetRef) -> eyre::Result<Multihash> {
        let mut resp = self
            .gql_api_call::<GetDatasetHeads>(get_dataset_heads::Variables {
                dataset_refs: vec![dataset_ref.clone()],
                skip_missing: false,
            })
            .await?;
        assert_eq!(resp.datasets.by_refs.len(), 1);
        Ok(resp.datasets.by_refs.pop().unwrap().head)
    }

    async fn sql_query<T: for<'de> Deserialize<'de>>(&self, sql: String) -> eyre::Result<T> {
        use sql_query::SqlQueryDataQuery;

        let response = self
            .gql_api_call::<SqlQuery>(sql_query::Variables {
                sql,
                // TODO: pagination if limit reached
                limit: i64::try_from(MAX_SQL_QUERY_LIMIT).unwrap(),
            })
            .await?;
        let raw_query_result = match response.data.query {
            SqlQueryDataQuery::DataQueryResultSuccess(query_result) => query_result,
            SqlQueryDataQuery::DataQueryResultError(e) => {
                bail!("Query failed with error: {e:#?}")
            }
        };
        let query_result: T = serde_json::from_str(&raw_query_result.data.content)?;

        Ok(query_result)
    }

    async fn gql_api_call<Q: GraphQLQuery>(
        &self,
        variables: Q::Variables,
    ) -> eyre::Result<Q::ResponseData> {
        self.metric_gql_requests_num_total.inc();

        let body = Q::build_query(variables);
        let response = self
            .http_client
            .post(&self.gql_api_endpoint)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        if status != StatusCode::OK {
            self.metric_gql_errors_num_total.inc();

            let body = response.text().await?;
            bail!("Unexpected status code: {status}, body: {body}");
        }

        let response: Response<Q::ResponseData> = response.json().await?;

        if let Some(data) = response.data {
            Ok(data)
        } else if let Some(errors) = response.errors {
            self.metric_gql_errors_num_total.inc();

            let error_message = errors.iter().map(ToString::to_string).collect::<Vec<_>>();
            bail!("Errors: {error_message:?}")
        } else {
            unreachable!()
        }
    }

    #[tracing::instrument(level = "debug", skip_all, fields(data_rooms_batch_size = data_rooms.len()))]
    async fn query_versioned_file_batch(
        &self,
        data_rooms: &[DataRoomDatasetIncrementalQuery],
    ) -> eyre::Result<Vec<VersionedFileEntryDto>> {
        let data_room_queries = data_rooms
            .iter()
            .map(|data_room| {
                let data_room_dataset_id = &data_room.dataset_id;
                let offset = data_room.last_seen_offset.map(|i| i + 1).unwrap_or(0);

                indoc::formatdoc!(
                    r#"
                    SELECT
                        '{data_room_dataset_id}' AS data_room_dataset_id,
                        "offset",
                        op,
                        path,
                        ref,
                        molecule_access_level
                    FROM '{data_room_dataset_id}'
                    WHERE offset >= {offset}
                    "#
                )
            })
            .collect::<Vec<_>>();

        let sql = indoc::formatdoc!(
            r#"
            SELECT data_room_dataset_id,
                   "offset",
                   op,
                   path,
                   ref,
                   molecule_access_level
            FROM ({subquery})
            ORDER BY data_room_dataset_id, offset
            "#,
            subquery = data_room_queries.join("UNION ALL\n")
        );

        self.sql_query::<Vec<VersionedFileEntryDto>>(sql).await
    }
}

#[async_trait]
impl KamuNodeApiClient for KamuNodeApiClientImpl {
    #[tracing::instrument(level = "debug", skip_all, fields(last_seen_offset, last_seen_head))]
    async fn get_molecule_project_entries<'a>(
        &self,
        last_seen_offset: Option<u64>,
        last_seen_head: Option<Multihash>,
        maybe_ignore_ocl_ids: Option<&'a HashSet<String>>,
    ) -> eyre::Result<(Multihash, Vec<MoleculeProjectEntry>)> {
        let molecule_projects = &self.molecule_projects_dataset_alias;

        let current_head = self.get_current_head(molecule_projects).await?;

        // Head didn't change?
        if Some(&current_head) == last_seen_head.as_ref() {
            return Ok((current_head, Vec::new()));
        }

        let offset = last_seen_offset.map(|i| i + 1).unwrap_or(0);

        // NOTE: We don't exclude retracted (-R) records. They are needed to correctly revoke permissions.
        let sql = indoc::formatdoc!(
            r#"
            SELECT offset,
                   op,
                   ocl_id,
                   symbol,
                   odf_account_id AS project_account_id,
                   odf_data_room_dataset_id AS data_room_dataset_id,
                   odf_announcements_dataset_id AS announcements_dataset_id
            FROM (SELECT *,
                         row_number() over (
                                         partition BY ocl_id
                                         ORDER BY `offset` DESC
                                     ) AS __rank
                  FROM '{molecule_projects}')
            WHERE __rank IN (1, 2) -- NOTE: include the last retracted records
              AND offset >= {offset}
            ORDER BY `offset`
            "#
        );

        let mut dtos = self.sql_query::<Vec<MoleculeProjectEntryDto>>(sql).await?;

        if let Some(ignore_ocl_ids) = maybe_ignore_ocl_ids {
            dtos.retain(|p| !ignore_ocl_ids.contains(&p.ocl_id));
        }

        let project_entries = dtos
            .into_iter()
            .map(TryInto::try_into)
            // Vec<Result<T, E>> --> Result<Vec<T>, E>
            .collect::<Result<Vec<MoleculeProjectEntry>, _>>()?;

        Ok((current_head, project_entries))
    }

    #[tracing::instrument(level = "debug", skip_all, fields(data_rooms_count = data_rooms.len()))]
    async fn get_versioned_files_entries_by_data_rooms(
        &self,
        data_rooms: Vec<DataRoomDatasetIncrementalQuery>,
    ) -> eyre::Result<VersionedFilesEntriesMap> {
        use futures::stream::{StreamExt, TryStreamExt};

        let mut result = VersionedFilesEntriesMap::new();

        if data_rooms.is_empty() {
            return Ok(result);
        }

        // Get current heads of data rooms.
        // We use this to:
        // - filter rooms that had no updates
        // - skip rooms that may have been deleted, not to crash SQL query
        let rooms_to_query = {
            let resolution = self
                .get_dataset_heads(
                    data_rooms
                        .iter()
                        .map(|data_room| data_room.dataset_id.clone())
                        .collect(),
                    true,
                )
                .await?;

            if !resolution.not_found.is_empty() {
                tracing::warn!(
                    "Some data rooms were not found (will be skipped during processing): {:?}",
                    resolution.not_found
                );
            }

            let mut rooms_to_query = Vec::<DataRoomDatasetIncrementalQuery>::new();

            for room in data_rooms {
                let Some(current_head) = resolution.resolved.get(&room.dataset_id) else {
                    continue;
                };

                result.insert(
                    room.dataset_id.clone(),
                    VersionedFilesEntries {
                        // NOTE: offset may change later as we scan data
                        latest_data_room_offset: room.last_seen_offset,
                        latest_data_room_head: current_head.clone(),
                        added_entities: Default::default(),
                        removed_entities: Default::default(),
                    },
                );

                if Some(current_head) != room.last_seen_head.as_ref() {
                    rooms_to_query.push(room);
                }
            }

            rooms_to_query
        };

        if rooms_to_query.is_empty() {
            return Ok(result);
        }

        let batch_ranges: Vec<_> =
            math::ranges::sub_ranges(rooms_to_query.len(), self.data_room_batch_size)
                .into_iter()
                .collect();
        let rooms_to_query_arc = Arc::new(rooms_to_query);

        let batch_results: Vec<Vec<VersionedFileEntryDto>> = futures::stream::iter(batch_ranges)
            .map(|batch_range| {
                // NOTE: To make borrow checker happy
                let q = Arc::clone(&rooms_to_query_arc);
                async move { self.query_versioned_file_batch(&q[batch_range]).await }
            })
            .buffer_unordered(self.max_concurrent_data_room_batches)
            .try_collect()
            .await?;

        let versioned_file_entry_dtos = batch_results.into_iter().flatten();

        for dto in versioned_file_entry_dtos {
            let entries = result.get_mut(&dto.data_room_dataset_id).unwrap();

            entries.latest_data_room_offset = Some(dto.offset);

            // Records that predate the molecule_access_level column have null here;
            // they are superseded by newer records with proper access levels.
            let Some(access_level) = dto.molecule_access_level else {
                continue;
            };

            let dataset_id = dto.r#ref;
            let entry = VersionedFileEntry {
                offset: dto.offset,
                path: dto.path,
                access_level,
            };

            let op: OperationType = dto.op.try_into()?;
            match op {
                OperationType::Append => {
                    entries.removed_entities.remove(&dataset_id);
                    entries.added_entities.insert(dataset_id, entry);
                }
                OperationType::Retract => {
                    entries.added_entities.remove(&dataset_id);
                    entries.removed_entities.insert(dataset_id, entry);
                }
                OperationType::CorrectFrom | OperationType::CorrectTo => {
                    entries.added_entities.insert(dataset_id, entry);
                }
            }
        }

        Ok(result)
    }

    #[tracing::instrument(level = "debug", skip_all, fields(did_pkhs_count = did_pkhs.len()))]
    async fn create_wallet_accounts(&self, did_pkhs: Vec<DidPhk>) -> eyre::Result<()> {
        if self.dry_run {
            return Ok(());
        }

        self.gql_api_call::<CreateWalletAccounts>(create_wallet_accounts::Variables {
            new_wallet_accounts: did_pkhs.iter().map(ToString::to_string).collect(),
        })
        .await?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operations_count = operations.len()))]
    async fn apply_account_dataset_relations(
        &self,
        operations: Vec<AccountDatasetRelationOperation>,
    ) -> eyre::Result<()> {
        if self.dry_run {
            return Ok(());
        }

        let operations = operations.into_iter().map(Into::into).collect();

        self.gql_api_call::<ApplyAccountDatasetRelations>(
            apply_account_dataset_relations::Variables { operations },
        )
        .await?;

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip_all, fields(datasets_count = dataset_ids.len()))]
    async fn get_dataset_heads(
        &self,
        dataset_ids: Vec<DatasetID>,
        skip_missing: bool,
    ) -> eyre::Result<DatasetResolution> {
        let response = self
            .gql_api_call::<GetDatasetHeads>(get_dataset_heads::Variables {
                dataset_refs: dataset_ids.clone(),
                skip_missing,
            })
            .await?;

        let resolved = response
            .datasets
            .by_refs
            .into_iter()
            .map(|item| (item.id, item.head))
            .collect::<std::collections::HashMap<_, _>>();

        let not_found = dataset_ids
            .iter()
            .filter(|id| !resolved.contains_key(*id))
            .cloned()
            .collect();

        Ok(DatasetResolution {
            resolved,
            not_found,
        })
    }
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "gql/schema.graphql",
    query_path = "gql/get_dataset_heads.graphql",
    response_derives = "Debug"
)]
struct GetDatasetHeads;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "gql/schema.graphql",
    query_path = "gql/sql_query.graphql",
    response_derives = "Debug"
)]
struct SqlQuery;

#[derive(Debug, Serialize, Deserialize)]
struct MoleculeProjectEntryDto {
    offset: u64,
    op: u8,
    ocl_id: String,
    symbol: String,
    project_account_id: AccountID,
    data_room_dataset_id: DatasetID,
    announcements_dataset_id: DatasetID,
}

impl TryInto<MoleculeProjectEntry> for MoleculeProjectEntryDto {
    type Error = eyre::Error;

    fn try_into(self) -> Result<MoleculeProjectEntry, Self::Error> {
        Ok(MoleculeProjectEntry {
            offset: self.offset,
            op: self.op.try_into()?,
            ocl_id: self.ocl_id.parse()?,
            symbol: self.symbol,
            project_account_id: self.project_account_id,
            data_room_dataset_id: self.data_room_dataset_id,
            announcements_dataset_id: self.announcements_dataset_id,
        })
    }
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "gql/schema.graphql",
    query_path = "gql/create_wallet_accounts.graphql",
    response_derives = "Debug"
)]
struct CreateWalletAccounts;

#[derive(Debug, Deserialize, Serialize)]
struct VersionedFileEntryDto {
    data_room_dataset_id: String,
    r#ref: String,
    offset: u64,
    op: u8,
    path: String,
    // Pre v2 API empty files were possible
    molecule_access_level: Option<MoleculeAccessLevel>,
}

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "gql/schema.graphql",
    query_path = "gql/apply_account_dataset_relations.graphql",
    response_derives = "Debug"
)]
struct ApplyAccountDatasetRelations;

impl From<AccountDatasetRelationOperation>
    for apply_account_dataset_relations::AccountDatasetRelationOperation
{
    fn from(v: AccountDatasetRelationOperation) -> Self {
        use apply_account_dataset_relations as codegen;

        Self {
            account_id: v.account_id,
            operation: match v.operation {
                DatasetRoleOperation::Set(role) => {
                    codegen::DatasetRoleOperation::Set(codegen::DatasetRoleSetOperation {
                        role: match role {
                            DatasetAccessRole::Reader => codegen::DatasetAccessRole::READER,
                            DatasetAccessRole::Maintainer => codegen::DatasetAccessRole::MAINTAINER,
                        },
                    })
                }
                DatasetRoleOperation::Unset => {
                    codegen::DatasetRoleOperation::Unset(codegen::DatasetRoleUnsetOperation {
                        dummy: false,
                    })
                }
            },
            dataset_id: v.dataset_id,
        }
    }
}
