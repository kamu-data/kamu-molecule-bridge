use std::sync::Arc;

use async_graphql::{Context, Object};
use serde_json::Value;

use crate::graphql::types::{Account, AccountID, Dataset, DatasetID};
use crate::http_server::StateRequester;

pub struct Query;

#[Object]
impl Query {
    /// Returns the current application state as JSON
    async fn state(&self, ctx: &Context<'_>) -> async_graphql::Result<Value> {
        let state_requester = ctx.data::<Arc<dyn StateRequester>>()?;
        let state_json = state_requester.request_as_json().await;
        Ok(state_json)
    }

    /// Returns API version information
    async fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    // TODO: remove after Molecule-specific GQL migration -->
    async fn dataset_from_molecule(&self, id: DatasetID) -> Dataset {
        Dataset { id }
    }

    async fn account_from_molecule(&self, id: AccountID) -> Account {
        Account { id }
    }
    // <--
}
