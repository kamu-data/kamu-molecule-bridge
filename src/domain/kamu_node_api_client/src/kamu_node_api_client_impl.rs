use std::collections::HashSet;
use std::str::FromStr;

use async_trait::async_trait;
use color_eyre::eyre;
use color_eyre::eyre::bail;
use graphql_client::{GraphQLQuery, Response};
use molecule_ipnft::entities::IpnftUid;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::did_phk::DidPhk;
use crate::{
    AccountDatasetRelationOperation, DataRoomDatasetIdWithOffset, DatasetAccessRole, DatasetID,
    DatasetRoleOperation, KamuNodeApiClient, MoleculeAccessLevel, MoleculeAccessLevelEntryMap,
    MoleculeProjectEntry, VersionedFileEntry, VersionedFilesEntriesMap,
};

pub struct KamuNodeApiClientImpl {
    gql_api_endpoint: String,
    token: String,
    molecule_projects_dataset_alias: String,
    http_client: reqwest::Client,
}

impl KamuNodeApiClientImpl {
    pub fn new(endpoint: String, token: String, molecule_projects_dataset_alias: String) -> Self {
        Self {
            gql_api_endpoint: endpoint,
            token,
            http_client: reqwest::Client::new(),
            molecule_projects_dataset_alias,
        }
    }

    async fn sql_query<T: for<'de> Deserialize<'de>>(&self, sql: String) -> eyre::Result<T> {
        use sql_query::SqlQueryDataQuery;

        let response = self
            .gql_api_call::<SqlQuery>(sql_query::Variables { sql })
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
            let body = response.text().await?;
            bail!("Unexpected status code: {status}, body: {body}",);
        }

        let response: Response<Q::ResponseData> = response.json().await?;

        if let Some(data) = response.data {
            Ok(data)
        } else if let Some(errors) = response.errors {
            let error_message = errors.iter().map(ToString::to_string).collect::<Vec<_>>();
            bail!("Errors: {error_message:?}")
        } else {
            unreachable!()
        }
    }
}

#[async_trait]
impl KamuNodeApiClient for KamuNodeApiClientImpl {
    #[tracing::instrument(level = "debug", skip_all, fields(offset = offset))]
    async fn get_molecule_project_entries(
        &self,
        offset: u64,
    ) -> eyre::Result<Vec<MoleculeProjectEntry>> {
        let molecule_projects = &self.molecule_projects_dataset_alias;

        // TODO: handle project deletions
        let sql = indoc::formatdoc!(
            r#"
            SELECT offset,
                   account_id AS project_account_id,
                   ipnft_uid,
                   ipnft_symbol,
                   data_room_dataset_id,
                   announcements_dataset_id
            FROM '{molecule_projects}'
            WHERE offset >= {offset}
            ORDER BY offset
            "#
        );

        let dtos = self.sql_query::<Vec<MoleculeProjectEntryDto>>(sql).await?;
        let project_entries = dtos
            .into_iter()
            .map(TryInto::try_into)
            // Vec<Result<T, E>> --> Result<Vec<T>, E>
            .collect::<Result<Vec<MoleculeProjectEntry>, _>>()?;

        Ok(project_entries)
    }

    #[tracing::instrument(level = "debug", skip_all, fields(data_rooms_count = data_rooms.len()))]
    async fn get_versioned_files_entries_by_data_rooms(
        &self,
        data_rooms: Vec<DataRoomDatasetIdWithOffset>,
    ) -> eyre::Result<VersionedFilesEntriesMap> {
        // NOTE: Since there might be data rooms with no records
        //       (and hence no data schema), we need to filter them out
        //       from the later query.
        let data_rooms_with_entries = {
            let data_room_has_entries_queries = data_rooms
                .iter()
                .map(|data_room| {
                    let data_room_dataset_id = &data_room.dataset_id;
                    indoc::formatdoc!(
                        r#"
                        SELECT '{data_room_dataset_id}' AS data_room_dataset_id,
                                COUNT(*) > 0 AS has_entries
                        FROM '{data_room_dataset_id}'
                        "#
                    )
                })
                .collect::<Vec<_>>();
            let sql = indoc::formatdoc!(
                r#"
                SELECT data_room_dataset_id
                FROM ({subquery})
                WHERE has_entries == TRUE
                "#,
                subquery = data_room_has_entries_queries.join("UNION ALL\n")
            );

            let data_rooms_with_entries = self
                .sql_query::<Vec<DataRoomWithEntriesDto>>(sql)
                .await?
                .into_iter()
                .map(|dto| dto.data_room_dataset_id)
                .collect::<HashSet<_>>();

            data_rooms
                .into_iter()
                .filter(|data_room| data_rooms_with_entries.contains(&data_room.dataset_id))
                .collect::<Vec<_>>()
        };

        let data_room_queries = data_rooms_with_entries
            .into_iter()
            .map(|data_room| {
                let data_room_dataset_id = data_room.dataset_id;
                let offset = data_room.offset;

                indoc::formatdoc!(
                    r#"
                    SELECT '{data_room_dataset_id}' AS data_room_dataset_id,
                           offset,
                           op,
                           path,
                           ref                      AS versioned_file_dataset_id
                    FROM '{data_room_dataset_id}'
                    WHERE offset >= {offset}
                    "#
                )
            })
            .collect::<Vec<_>>();

        let sql = indoc::formatdoc!(
            r#"
            SELECT data_room_dataset_id,
                   offset,
                   op,
                   path,
                   versioned_file_dataset_id
            FROM ({subquery})
            ORDER BY data_room_dataset_id, offset
            "#,
            subquery = data_room_queries.join("UNION ALL\n")
        );

        let versioned_file_entry_dtos = self.sql_query::<Vec<VersionedFileEntryDto>>(sql).await?;

        let mut versioned_files_entries_map = VersionedFilesEntriesMap::new();
        for dto in versioned_file_entry_dtos {
            let data_room_entries = versioned_files_entries_map
                .entry(dto.data_room_dataset_id)
                .or_default();

            data_room_entries.latest_data_room_offset = dto.offset;

            let dataset_id = dto.versioned_file_dataset_id;
            let entry = VersionedFileEntry {
                offset: dto.offset,
                path: dto.path,
            };

            let op: OperationType = dto.op.try_into()?;
            match op {
                OperationType::Append => {
                    data_room_entries.removed_entities.remove(&dataset_id);
                    data_room_entries.added_entities.insert(dataset_id, entry);
                }
                OperationType::Retract => {
                    data_room_entries.added_entities.remove(&dataset_id);
                    data_room_entries.removed_entities.insert(dataset_id, entry);
                }
                OperationType::CorrectFrom | OperationType::CorrectTo => {
                    // TODO: do we need reaction here?
                }
            }
        }

        Ok(versioned_files_entries_map)
    }

    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(
            versioned_file_dataset_ids_count = versioned_file_dataset_ids.len()
        )
    )]
    async fn get_latest_molecule_access_levels_by_dataset_ids(
        &self,
        versioned_file_dataset_ids: Vec<String>,
    ) -> eyre::Result<MoleculeAccessLevelEntryMap> {
        // NOTE: Since there might be versioned files with no records
        //       (for example, just created), we need to filter them out
        //       from the later query.
        let versioned_files_with_entries = {
            let versioned_file_has_entries_queries = versioned_file_dataset_ids
                .iter()
                .map(|versioned_file_dataset_id| {
                    indoc::formatdoc!(
                        r#"
                        SELECT '{versioned_file_dataset_id}' AS versioned_file_dataset_id,
                                COUNT(*) > 0 AS has_entries
                        FROM '{versioned_file_dataset_id}'
                        "#
                    )
                })
                .collect::<Vec<_>>();
            let sql = indoc::formatdoc!(
                r#"
                SELECT versioned_file_dataset_id
                FROM ({subquery})
                WHERE has_entries == TRUE
                "#,
                subquery = versioned_file_has_entries_queries.join("UNION ALL\n")
            );

            let versioned_files_with_entries = self
                .sql_query::<Vec<VersionedFileWithEntriesDto>>(sql)
                .await?
                .into_iter()
                .map(|dto| dto.versioned_file_dataset_id)
                .collect::<HashSet<_>>();

            versioned_file_dataset_ids
                .into_iter()
                .filter(|dataset_id| versioned_files_with_entries.contains(dataset_id))
                .collect::<Vec<_>>()
        };

        let molecule_access_level_queries = versioned_files_with_entries
            .iter()
            .map(|versioned_file_dataset_id| {
                indoc::formatdoc!(
                    r#"
                    (SELECT '{versioned_file_dataset_id}' as versioned_file_dataset_id,
                            molecule_access_level
                     FROM '{versioned_file_dataset_id}'
                     ORDER BY offset DESC
                     LIMIT 1)
                    "#
                )
            })
            .collect::<Vec<_>>();
        let sql = indoc::formatdoc!(
            r#"
            SELECT versioned_file_dataset_id,
                   molecule_access_level
            FROM ({subquery})
            "#,
            subquery = molecule_access_level_queries.join("UNION ALL\n")
        );

        let dtos = self
            .sql_query::<Vec<VersionedFileMoleculeAccessLevelDto>>(sql)
            .await?;
        let map = dtos
            .into_iter()
            .map(|dto| (dto.versioned_file_dataset_id, dto.molecule_access_level))
            .collect();

        Ok(map)
    }

    #[tracing::instrument(level = "debug", skip_all, fields(did_pkhs_count = did_pkhs.len()))]
    async fn create_wallet_accounts(&self, did_pkhs: Vec<DidPhk>) -> color_eyre::Result<()> {
        // TODO: batches? we have ~700 holders for some IPNFT

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
    ) -> color_eyre::Result<()> {
        // TODO: batches? we have ~1400 operations for some IPNFT

        let operations = operations.into_iter().map(Into::into).collect();

        self.gql_api_call::<ApplyAccountDatasetRelations>(
            apply_account_dataset_relations::Variables { operations },
        )
        .await?;

        Ok(())
    }
}

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
    ipnft_uid: String,
    ipnft_symbol: String,
    project_account_id: crate::AccountID,
    data_room_dataset_id: DatasetID,
    announcements_dataset_id: DatasetID,
}

impl TryInto<MoleculeProjectEntry> for MoleculeProjectEntryDto {
    type Error = eyre::Error;

    fn try_into(self) -> Result<MoleculeProjectEntry, Self::Error> {
        Ok(MoleculeProjectEntry {
            offset: self.offset,
            ipnft_uid: IpnftUid::from_str(&self.ipnft_uid)?,
            symbol: self.ipnft_symbol,
            project_account_id: self.project_account_id,
            data_room_dataset_id: self.data_room_dataset_id,
            announcements_dataset_id: self.announcements_dataset_id,
        })
    }
}

// NOTE: GQL scalars require additional declarations
type DidPkh = String;
type AccountID = String;
#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "gql/schema.graphql",
    query_path = "gql/create_wallet_accounts.graphql",
    response_derives = "Debug"
)]
struct CreateWalletAccounts;

#[derive(Debug, Deserialize, Serialize)]
struct DataRoomWithEntriesDto {
    data_room_dataset_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct VersionedFileEntryDto {
    data_room_dataset_id: String,
    offset: u64,
    op: u8,
    versioned_file_dataset_id: String,
    path: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct VersionedFileWithEntriesDto {
    versioned_file_dataset_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct VersionedFileMoleculeAccessLevelDto {
    versioned_file_dataset_id: String,
    molecule_access_level: MoleculeAccessLevel,
}

#[repr(u8)]
#[derive(Debug)]
enum OperationType {
    Append = 0,
    Retract = 1,
    CorrectFrom = 2,
    CorrectTo = 3,
}

impl TryFrom<u8> for OperationType {
    type Error = eyre::Error;

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        let op = match v {
            0 => OperationType::Append,
            1 => OperationType::Retract,
            2 => OperationType::CorrectFrom,
            3 => OperationType::CorrectTo,
            unexpected => bail!("Unexpected operation type: {unexpected}"),
        };
        Ok(op)
    }
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
