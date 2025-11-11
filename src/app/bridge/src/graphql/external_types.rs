use async_graphql::{SimpleObject, scalar};
use chrono::{DateTime, Utc};

// Kamu Core API: external types

#[nutype::nutype(derive(Serialize, Deserialize, Clone))]
pub struct AccountID(String);

scalar!(AccountID);

#[derive(SimpleObject)]
#[graphql(unresolvable)]
pub struct Account {
    pub id: AccountID,
}

#[nutype::nutype(derive(Serialize, Deserialize, Clone))]
pub struct DatasetID(String);

scalar!(DatasetID);

#[derive(SimpleObject)]
#[graphql(unresolvable)]
pub struct Dataset {
    pub id: DatasetID,
}

#[nutype::nutype(derive(Serialize, Deserialize, Clone))]
pub struct CollectionPath(String);

scalar!(CollectionPath);

#[derive(SimpleObject)]
#[graphql(unresolvable)]
pub struct CollectionEntry {
    // NOTE: These fields will be used to get this entity in the Kamu Core API subgraph -->
    pub data_room_dataset_id: DatasetID,
    pub entry_path: CollectionPath,
    // <--
    #[graphql(external)]
    pub system_time: DateTime<Utc>,
}

#[derive(SimpleObject)]
#[graphql(unresolvable)]
pub struct VersionedFileEntry {
    pub id: DatasetID,
    #[graphql(external)]
    pub system_time: DateTime<Utc>,
}
