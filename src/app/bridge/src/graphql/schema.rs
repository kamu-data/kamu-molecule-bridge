use async_graphql::{EmptyMutation, EmptySubscription};

use crate::graphql::query::QueryRoot;

pub type Schema = async_graphql::Schema<QueryRoot, EmptyMutation, EmptySubscription>;
pub type SchemaBuilder = async_graphql::SchemaBuilder<QueryRoot, EmptyMutation, EmptySubscription>;

pub fn schema_builder() -> SchemaBuilder {
    Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .extension(async_graphql::extensions::Tracing)
        .enable_federation()
}
