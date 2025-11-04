use crate::graphql;

pub fn router(endpoint: &'static str) -> axum::Router {
    axum::Router::new().route(
        endpoint,
        axum::routing::get(async move || graphql_playground_handler_builder(endpoint))
            .post(graphql_handler),
    )
}

async fn graphql_handler(
    axum::extract::Extension(schema): axum::extract::Extension<graphql::Schema>,
    req: async_graphql_axum::GraphQLRequest,
) -> async_graphql_axum::GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

fn graphql_playground_handler_builder(endpoint: &'static str) -> axum::response::Html<String> {
    axum::response::Html(
        async_graphql::http::GraphiQLSource::build()
            .endpoint(endpoint)
            .finish(),
    )
}
