use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use eyre::bail;
use molecule_ocl::entities::OclId;
use serde::{Deserialize, Serialize};

use crate::did_phk::DidPhk;

#[cfg_attr(
    any(feature = "testing", test),
    mockall::automock,
    allow(clippy::ref_option_ref)
)]
#[async_trait]
pub trait KamuNodeApiClient {
    async fn get_molecule_project_entries<'a>(
        &self,
        last_seen_offset: Option<u64>,
        last_seen_head: Option<Multihash>,
        maybe_ignore_ocl_ids: Option<&'a HashSet<String>>,
    ) -> eyre::Result<(Multihash, Vec<MoleculeProjectEntry>)>;

    async fn get_versioned_files_entries_by_data_rooms(
        &self,
        data_rooms: Vec<DataRoomDatasetIncrementalQuery>,
    ) -> eyre::Result<VersionedFilesEntriesMap>;

    async fn create_wallet_accounts(&self, did_pkhs: Vec<DidPhk>) -> eyre::Result<()>;

    async fn apply_account_dataset_relations(
        &self,
        operations: Vec<AccountDatasetRelationOperation>,
    ) -> eyre::Result<()>;

    async fn get_dataset_heads(
        &self,
        dataset_ids: Vec<DatasetID>,
        skip_missing: bool,
    ) -> eyre::Result<DatasetResolution>;
}

pub type DatasetID = String;
pub type DatasetRef = String;
pub type AccountID = String;
pub type Multihash = String;
pub type DidPkh = String;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum OperationType {
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

#[derive(Debug, Serialize)]
pub struct MoleculeProjectEntry {
    pub offset: u64,
    pub op: OperationType,
    pub ocl_id: OclId,
    pub symbol: String,
    pub project_account_id: AccountID,
    pub data_room_dataset_id: DatasetID,
    pub announcements_dataset_id: DatasetID,
}

impl MoleculeProjectEntry {
    pub fn is_deleted(&self) -> bool {
        self.op == OperationType::Retract
    }
}

pub type VersionedFilesEntriesMap =
    HashMap</* data_room_dataset_id */ DatasetID, VersionedFilesEntries>;

#[derive(Debug)]
pub struct VersionedFilesEntries {
    pub latest_data_room_offset: Option<u64>,
    pub latest_data_room_head: String,

    pub added_entities: ChangedVersionedFiles,
    pub removed_entities: ChangedVersionedFiles,
}

pub type ChangedVersionedFiles = HashMap<DatasetID, VersionedFileEntry>;

#[derive(Debug, Serialize)]
pub struct VersionedFileEntry {
    pub offset: u64,
    pub path: String,
    pub access_level: MoleculeAccessLevel,
}

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
    // NOTE: plural variants only occur for molecule.dev / testnet
    #[serde(alias = "HOLDER", alias = "holders", alias = "HOLDERS")]
    Holder,
}

#[derive(Debug)]
pub struct DataRoomDatasetIncrementalQuery {
    pub dataset_id: DatasetID,
    pub last_seen_offset: Option<u64>,
    pub last_seen_head: Option<Multihash>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountDatasetRelationOperation {
    pub account_id: AccountID,
    pub operation: DatasetRoleOperation,
    pub dataset_id: DatasetID,
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

#[derive(Debug)]
pub struct DatasetResolution {
    pub resolved: HashMap<DatasetID, Multihash>,
    pub not_found: HashSet<DatasetID>,
}
