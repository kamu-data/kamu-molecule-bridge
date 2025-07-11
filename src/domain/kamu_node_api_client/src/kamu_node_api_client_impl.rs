use async_trait::async_trait;
use color_eyre::eyre;
use color_eyre::eyre::bail;
use graphql_client::{GraphQLQuery, Response};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::kamu_node_api_client_impl::molecule_projects_view::MoleculeProjectsViewDataQuery;
use crate::{
    KamuNodeApiClient,
    MoleculeAccessLevelEntry,
    MoleculeProjectEntry,
    VersionedFileEntry,
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

    async fn gql_api_call<Q: GraphQLQuery>(
        &self,
        operation_name: &str,
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
            // TODO: tracing operation_name instead of inlining into an error?
            bail!("[{operation_name}]: Unexpected status code: {status}, body: {body}",);
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
        let molecule_projects = self.molecule_projects_dataset_alias.clone();
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

        let response = self
            .gql_api_call::<MoleculeProjectsView>(
                molecule_projects_view::OPERATION_NAME,
                molecule_projects_view::Variables { sql },
            )
            .await?;
        let query_result = match response.data.query {
            MoleculeProjectsViewDataQuery::DataQueryResultSuccess(query_result) => query_result,
            MoleculeProjectsViewDataQuery::DataQueryResultError(e) => {
                bail!("Query failed with error: {e:#?}")
            }
        };

        let dtos: Vec<MoleculeProjectsEntryDto> = serde_json::from_str(&query_result.data.content)?;
        let molecule_project_entries = dtos
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(molecule_project_entries)
    }

    async fn get_versioned_files_entries_by_ipnft_uid(
        &self,
        _ipnft_uid: &str,
        _project_dataset_head: Option<String>,
    ) -> eyre::Result<Vec<VersionedFileEntry>> {
        todo!()
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
    query_path = "gql/molecule_projects_view.graphql",
    response_derives = "Debug"
)]
struct MoleculeProjectsView;

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
