use async_graphql::{EmptyMutation, EmptySubscription};

use crate::graphql::query::Query;

pub type Schema = async_graphql::Schema<Query, EmptyMutation, EmptySubscription>;
pub type SchemaBuilder = async_graphql::SchemaBuilder<Query, EmptyMutation, EmptySubscription>;

pub fn schema_builder() -> SchemaBuilder {
    Schema::build(Query, EmptyMutation, EmptySubscription)
        .extension(async_graphql::extensions::Tracing)
        .enable_federation()
}
