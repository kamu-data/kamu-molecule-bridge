pub mod external_types;
pub mod handlers;
pub mod mutations;
pub mod prelude;
pub mod queries;
pub mod root;
pub mod scalars;
pub mod schema;

pub use handlers::router;
pub use schema::{Schema, schema_builder};
