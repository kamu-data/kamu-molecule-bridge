use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use color_eyre::eyre;
use serde::{Deserialize, Serialize};

use crate::did_phk::DidPhk;

#[cfg_attr(any(feature = "testing", test), mockall::automock)]
#[async_trait]
pub trait KamuNodeApiClient {
    async fn get_molecule_project_entries(
        &self,
        maybe_offset: Option<u64>,
    ) -> eyre::Result<Vec<MoleculeProjectEntry>>;

    async fn get_versioned_files_entries_by_data_rooms(
        &self,
        data_rooms: Vec<DataRoomDatasetIdWithOffset>,
    ) -> eyre::Result<VersionedFilesEntriesMap>;

    async fn get_latest_molecule_access_levels_by_dataset_ids(
        &self,
        versioned_file_dataset_ids: Vec<String>,
    ) -> eyre::Result<MoleculeAccessLevelEntryMap>;

    async fn create_wallet_accounts(&self, did_pkhs: Vec<DidPhk>) -> eyre::Result<()>;

    async fn apply_account_dataset_relations(
        &self,
        operations: Vec<AccountDatasetRelationOperation>,
    ) -> eyre::Result<()>;
}

pub type DatasetID = String;
pub type AccountID = String;

#[derive(Debug, Serialize, Deserialize)]
pub struct MoleculeProjectEntry {
    pub offset: u64,
    // TODO: use type?
    pub ipnft_uid: String,
    pub project_account_id: AccountID,
    pub data_room_dataset_id: DatasetID,
    pub announcements_dataset_id: DatasetID,
}

pub type VersionedFilesEntriesMap =
    HashMap</* data_room_dataset_id */ DatasetID, VersionedFilesEntries>;

#[derive(Debug, Default)]
pub struct VersionedFilesEntries {
    pub latest_data_room_offset: u64,
    pub added_entities: ChangedVersionedFiles,
    pub removed_entities: ChangedVersionedFiles,
}

pub type ChangedVersionedFiles = HashSet</* versioned_file_dataset_id */ DatasetID>;

pub type MoleculeAccessLevelEntryMap =
    HashMap</* versioned_file_dataset_id */ DatasetID, MoleculeAccessLevel>;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MoleculeAccessLevel {
    Public,
    Admin,
    #[serde(rename = "admin_2")]
    Admin2,
    Holder,
}

#[derive(Debug)]
pub struct DataRoomDatasetIdWithOffset {
    pub dataset_id: DatasetID,
    pub offset: Option<u64>,
}

#[derive(Debug)]
pub struct AccountDatasetRelationOperation {
    pub account_id: DatasetID,
    pub operation: DatasetRoleOperation,
    pub dataset_id: AccountID,
}

#[derive(Debug, Copy, Clone)]
pub enum DatasetRoleOperation {
    Set(DatasetAccessRole),
    Unset,
}

#[derive(Debug, Copy, Clone)]
pub enum DatasetAccessRole {
    Reader,
    Maintainer,
}
