use async_trait::async_trait;
use color_eyre::eyre;
use color_eyre::eyre::bail;

#[cfg_attr(any(feature = "testing", test), mockall::automock)]
#[async_trait]
pub trait KamuNodeApiClient {
    async fn get_molecule_project_entries(
        &self,
        maybe_offset: Option<u64>,
    ) -> eyre::Result<Vec<MoleculeProjectEntry>>;

    async fn get_versioned_files_entries_by_ipnft_uid(
        &self,
        ipnft_uid: &str,
        project_dataset_head: Option<String>,
    ) -> eyre::Result<Vec<VersionedFileEntry>>;

    async fn get_latest_molecule_access_levels_by_dataset_ids(
        &self,
        dataset_ids: Vec<String>,
    ) -> eyre::Result<Vec<MoleculeAccessLevelEntry>>;
}

#[derive(Debug)]
pub struct MoleculeProjectEntry {
    pub offset: u64,
    pub op: OperationType,
    // TODO: use type?
    pub ipnft_uid: String,
    pub data_room_dataset_id: String,
    pub announcements_dataset_id: String,
}

#[derive(Debug)]
pub struct VersionedFileEntry {
    pub ipnft_uid: String,
    pub dataset_id: String,
    pub op: String,
}

#[derive(Debug)]
pub struct MoleculeAccessLevelEntry {
    pub dataset_id: String,
    // TODO: extract type
    pub access_level: String,
}

#[repr(u8)]
#[derive(Debug)]
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
