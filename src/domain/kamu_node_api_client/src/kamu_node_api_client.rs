use async_trait::async_trait;
use color_eyre::eyre;

#[cfg_attr(any(feature = "testing", test), mockall::automock)]
#[async_trait]
pub trait KamuNodeApiClient {
    async fn get_molecule_projects_entries(
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

pub struct MoleculeProjectEntry {
    pub offset: u64,
    // TODO: extract type
    pub op: String,
    // TODO: use type?
    pub ipnft_uid: String,
    pub data_room_dataset_id: String,
    pub announcements_dataset_id: String,
}

pub struct VersionedFileEntry {
    pub ipnft_uid: String,
    pub dataset_id: String,
    pub op: String,
}

pub struct MoleculeAccessLevelEntry {
    pub dataset_id: String,
    // TODO: extract type
    pub access_level: String,
}
