use std::collections::HashMap;

use async_trait::async_trait;
use color_eyre::eyre;
use molecule_ipnft::entities::IpnftUid;
use serde::{Deserialize, Serialize};

use crate::did_phk::DidPhk;

#[cfg_attr(any(feature = "testing", test), mockall::automock)]
#[async_trait]
pub trait KamuNodeApiClient {
    async fn get_molecule_project_entries(
        &self,
        offset: u64,
        ignore_ipnft_uids: &std::collections::HashSet<String>,
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

#[derive(Debug, Serialize)]
pub struct MoleculeProjectEntry {
    pub offset: u64,
    pub op: u8,
    pub ipnft_uid: IpnftUid,
    pub symbol: String,
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

pub type ChangedVersionedFiles = HashMap<DatasetID, VersionedFileEntry>;

#[derive(Debug, Serialize)]
pub struct VersionedFileEntry {
    pub offset: u64,
    pub path: String,
}

pub type MoleculeAccessLevelEntryMap =
    HashMap</* versioned_file_dataset_id */ DatasetID, MoleculeAccessLevel>;

// https://discord.com/channels/@me/1364902681159794688/1394272024746135644
#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MoleculeAccessLevel {
    #[serde(alias = "PUBLIC")]
    Public,
    #[serde(alias = "ADMIN")]
    Admin,
    #[serde(rename = "admin_2", alias = "ADMIN_2")]
    Admin2,
    #[serde(alias = "HOLDER")]
    Holder,
}

#[derive(Debug)]
pub struct DataRoomDatasetIdWithOffset {
    pub dataset_id: DatasetID,
    pub offset: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountDatasetRelationOperation {
    pub account_id: DatasetID,
    pub operation: DatasetRoleOperation,
    pub dataset_id: AccountID,
}

impl AccountDatasetRelationOperation {
    pub fn reader_access(account_id: AccountID, dataset_id: DatasetID) -> Self {
        Self {
            account_id,
            operation: DatasetRoleOperation::Set(DatasetAccessRole::Reader),
            dataset_id,
        }
    }

    pub fn maintainer_access(account_id: AccountID, dataset_id: DatasetID) -> Self {
        Self {
            account_id,
            operation: DatasetRoleOperation::Set(DatasetAccessRole::Maintainer),
            dataset_id,
        }
    }

    pub fn revoke_access(account_id: AccountID, dataset_id: DatasetID) -> Self {
        Self {
            account_id,
            operation: DatasetRoleOperation::Unset,
            dataset_id,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize)]
pub enum DatasetRoleOperation {
    Set(DatasetAccessRole),
    Unset,
}

#[derive(Debug, Copy, Clone, Serialize)]
pub enum DatasetAccessRole {
    Reader,
    Maintainer,
}
