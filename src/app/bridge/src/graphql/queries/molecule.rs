use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::graphql::prelude::*;

pub struct Molecule;

#[common_macros::method_names_consts(const_value_prefix = "Gql::")]
#[Object]
impl Molecule {
    const DEFAULT_PROJECTS_PER_PAGE: usize = 15;
    const DEFAULT_ACTIVITY_EVENTS_PER_PAGE: usize = 15;

    /// Looks up the project
    #[tracing::instrument(level = "info", name = Molecule_project, skip_all, fields(?ipnft_uid))]
    async fn project(
        &self,
        _ctx: &Context<'_>,
        ipnft_uid: String,
    ) -> GqlResult<Option<MoleculeProject>> {
        // todo
        Err(GqlError::new("Not implemented"))
    }

    /// List the registered projects
    #[tracing::instrument(level = "info", name = Molecule_projects, skip_all)]
    async fn projects(
        &self,
        _ctx: &Context<'_>,
        _page: Option<usize>,
        _per_page: Option<usize>,
    ) -> GqlResult<MoleculeProjectConnection> {
        // todo
        Err(GqlError::new("Not implemented"))
    }

    /// Latest activity events across all projects in reverse chronological
    /// order
    #[tracing::instrument(level = "info", name = Molecule_activity, skip_all)]
    async fn activity(
        &self,
        _ctx: &Context<'_>,
        _page: Option<usize>,
        _per_page: Option<usize>,
    ) -> GqlResult<MoleculeProjectEventConnection> {
        Err(GqlError::new("Not implemented"))
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct MoleculeProject {
    #[graphql(skip)]
    pub account_id: AccountID,

    /// System time when this version was created/updated
    pub system_time: DateTime<Utc>,

    /// Event time when this version was created/updated
    pub event_time: DateTime<Utc>,

    /// Symbolic name of the project
    pub ipnft_symbol: String,

    /// Unique ID of the IPNFT as `{ipnftAddress}_{ipnftTokenId}`
    pub ipnft_uid: String,

    /// Address of the IPNFT contract
    pub ipnft_address: String,

    // NOTE: For backward compatibility (and existing projects),
    //       we continue using BigInt type, which is wider than needed U256.
    /// Token ID withing the IPNFT contract
    pub ipnft_token_id: BigInt,

    #[graphql(skip)]
    pub data_room_dataset_id: DatasetID,

    #[graphql(skip)]
    pub announcements_dataset_id: DatasetID,
}

#[common_macros::method_names_consts(const_value_prefix = "Gql::")]
#[ComplexObject]
impl MoleculeProject {
    const DEFAULT_ACTIVITY_EVENTS_PER_PAGE: usize = 15;

    /// Project's organizational account
    #[tracing::instrument(level = "info", name = MoleculeProject_account, skip_all)]
    async fn account(&self, _ctx: &Context<'_>) -> GqlResult<Account> {
        // todo
        Err(GqlError::new("Not implemented"))
    }

    /// Project's data room dataset
    #[tracing::instrument(level = "info", name = MoleculeProject_data_room, skip_all)]
    async fn data_room(&self, _ctx: &Context<'_>) -> GqlResult<Dataset> {
        // todo
        Err(GqlError::new("Not implemented"))
    }

    /// Project's announcements dataset
    #[tracing::instrument(level = "info", name = MoleculeProject_announcements, skip_all)]
    async fn announcements(&self, _ctx: &Context<'_>) -> GqlResult<Dataset> {
        // todo
        Err(GqlError::new("Not implemented"))
    }

    /// Project's activity events in reverse chronological order
    #[tracing::instrument(level = "info", name = MoleculeProject_activity, skip_all)]
    async fn activity(
        &self,
        _ctx: &Context<'_>,
        _page: Option<usize>,
        _per_page: Option<usize>,
    ) -> GqlResult<MoleculeProjectEventConnection> {
        // todo
        Err(GqlError::new("Not implemented"))
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

page_based_connection!(
    MoleculeProject,
    MoleculeProjectConnection,
    MoleculeProjectEdge
);

////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Interface)]
#[graphql(field(name = "project", ty = "&Arc<MoleculeProject>"))]
#[graphql(field(name = "system_time", ty = "DateTime<Utc>"))]
pub enum MoleculeProjectEvent {
    DataRoomEntryAdded(MoleculeProjectEventDataRoomEntryAdded),
    DataRoomEntryRemoved(MoleculeProjectEventDataRoomEntryRemoved),
    DataRoomEntryUpdated(MoleculeProjectEventDataRoomEntryUpdated),
    Announcement(MoleculeProjectEventAnnouncement),
    FileUpdated(MoleculeProjectEventFileUpdated),
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct MoleculeProjectEventDataRoomEntryAdded {
    /// Associated project
    pub project: Arc<MoleculeProject>,
    /// Collection entry
    pub entry: CollectionEntry,
}
#[ComplexObject]
impl MoleculeProjectEventDataRoomEntryAdded {
    async fn system_time(&self) -> DateTime<Utc> {
        self.entry.system_time
    }
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct MoleculeProjectEventDataRoomEntryRemoved {
    /// Associated project
    pub project: Arc<MoleculeProject>,
    /// Collection entry
    pub entry: CollectionEntry,
}
#[ComplexObject]
impl MoleculeProjectEventDataRoomEntryRemoved {
    async fn system_time(&self) -> DateTime<Utc> {
        self.entry.system_time
    }
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct MoleculeProjectEventDataRoomEntryUpdated {
    /// Associated project
    pub project: Arc<MoleculeProject>,
    /// Collection entry
    pub new_entry: CollectionEntry,
}
#[ComplexObject]
impl MoleculeProjectEventDataRoomEntryUpdated {
    async fn system_time(&self) -> DateTime<Utc> {
        self.new_entry.system_time
    }
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct MoleculeProjectEventAnnouncement {
    /// Associated project
    pub project: Arc<MoleculeProject>,
    /// Announcement record
    pub announcement: serde_json::Value,
}
#[ComplexObject]
impl MoleculeProjectEventAnnouncement {
    async fn system_time(&self) -> DateTime<Utc> {
        // todo
        DateTime::default()
    }
}

#[derive(SimpleObject)]
#[graphql(complex)]
pub struct MoleculeProjectEventFileUpdated {
    /// Associated project
    pub project: Arc<MoleculeProject>,

    /// Versioned file dataset
    pub dataset: Dataset,

    /// New file version entry
    pub new_entry: VersionedFileEntry,
}
#[ComplexObject]
impl MoleculeProjectEventFileUpdated {
    async fn system_time(&self) -> DateTime<Utc> {
        self.new_entry.system_time
    }
}

page_based_stream_connection!(
    MoleculeProjectEvent,
    MoleculeProjectEventConnection,
    MoleculeProjectEventEdge
);
