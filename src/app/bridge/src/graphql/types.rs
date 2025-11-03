use async_graphql::{SimpleObject, scalar};

// Kamu Core API: external types

#[nutype::nutype(derive(Serialize, Deserialize))]
pub struct AccountID(String);

scalar!(AccountID);

#[derive(SimpleObject)]
#[graphql(unresolvable)]
pub struct Account {
    pub id: AccountID,
}

#[nutype::nutype(derive(Serialize, Deserialize))]
pub struct DatasetID(String);

scalar!(DatasetID);

#[derive(SimpleObject)]
#[graphql(unresolvable)]
pub struct Dataset {
    pub id: DatasetID,
}
