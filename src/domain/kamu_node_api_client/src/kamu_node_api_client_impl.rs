use std::collections::HashSet;

use async_trait::async_trait;
use color_eyre::eyre;
use color_eyre::eyre::bail;
use graphql_client::{GraphQLQuery, Response};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{
    DataRoomDatasetIdWithOffset,
    KamuNodeApiClient,
    MoleculeAccessLevelEntry,
    MoleculeProjectEntry,
    VersionedFileEntry,
    VersionedFilesEntriesMap,
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
        use crate::kamu_node_api_client_impl::sql_query::SqlQueryDataQuery;

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
    async fn get_molecule_project_entries(
        &self,
        maybe_offset: Option<u64>,
    ) -> eyre::Result<Vec<MoleculeProjectEntry>> {
        let molecule_projects = &self.molecule_projects_dataset_alias;
        let offset = maybe_offset.unwrap_or(0);

        let sql = indoc::formatdoc!(
            r#"
            SELECT offset,
                   op,
                   ipnft_uid,
                   data_room_dataset_id,
                   announcements_dataset_id
            FROM '{molecule_projects}'
            WHERE offset >= {offset}
            ORDER BY offset
            "#
        );

        let response = self.sql_query::<Vec<MoleculeProjectsEntryDto>>(sql).await?;
        let molecule_project_entries = response
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(molecule_project_entries)
    }

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
                .sql_query::<Vec<DataRoomWithEntryDto>>(sql)
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
                let offset = data_room.offset.unwrap_or(0);

                indoc::formatdoc!(
                    r#"
                    SELECT '{data_room_dataset_id}' AS data_room_dataset_id,
                           offset,
                           op,
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

            data_room_entries.push(VersionedFileEntry {
                offset: dto.offset,
                op: dto.op.try_into()?,
                dataset_id: dto.versioned_file_dataset_id,
            });
        }

        Ok(versioned_files_entries_map)
    }

    async fn get_latest_molecule_access_levels_by_dataset_ids(
        &self,
        _dataset_ids: Vec<String>,
    ) -> eyre::Result<Vec<MoleculeAccessLevelEntry>> {
        todo!()
    }
}

// TODO: Add build.rs: rebuild if *.graphql files changed
#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "gql/schema.graphql",
    query_path = "gql/sql_query.graphql",
    response_derives = "Debug"
)]
struct SqlQuery;

#[derive(Debug, Deserialize, Serialize)]
struct MoleculeProjectsEntryDto {
    offset: u64,
    op: u8,
    ipnft_uid: String,
    data_room_dataset_id: String,
    announcements_dataset_id: String,
}

impl TryFrom<MoleculeProjectsEntryDto> for MoleculeProjectEntry {
    type Error = eyre::Error;

    fn try_from(v: MoleculeProjectsEntryDto) -> Result<Self, Self::Error> {
        Ok(Self {
            offset: v.offset,
            op: v.op.try_into()?,
            ipnft_uid: v.ipnft_uid,
            data_room_dataset_id: v.data_room_dataset_id,
            announcements_dataset_id: v.announcements_dataset_id,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct DataRoomWithEntryDto {
    data_room_dataset_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct VersionedFileEntryDto {
    data_room_dataset_id: String,
    offset: u64,
    op: u8,
    versioned_file_dataset_id: String,
}
