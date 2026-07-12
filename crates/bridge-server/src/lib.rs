//! Axum webhook server: configuration, engine assembly and the Postgres
//! adapter for the dialogue engine defined in `bridge-core`.

pub mod assemble;
pub mod config;
pub mod routes;
pub mod store_pg;
