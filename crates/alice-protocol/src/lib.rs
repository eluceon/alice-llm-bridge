//! Typed models for the Yandex Dialogs webhook protocol.
//!
//! Pure data definitions with serde only: no HTTP, no business logic.

mod request;

pub use request::{
    AccountUser, Application, Meta, RequestKind, Session, UserRequest, WebhookRequest,
};
