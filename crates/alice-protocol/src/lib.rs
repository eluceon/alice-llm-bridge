//! Typed models for the Yandex Dialogs webhook protocol.
//!
//! Pure data definitions with serde only: no HTTP, no business logic.

mod request;
mod response;

pub use request::{
    AccountUser, Application, Meta, RequestKind, Session, UserRequest, WebhookRequest,
};
pub use response::{MAX_TEXT_LEN, SkillResponse, WebhookResponse, clip_to_limit};
