//! The webhook route: access control, greeting and dispatch into the engine.

use std::collections::HashSet;

use alice_protocol::{WebhookRequest, WebhookResponse};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bridge_core::{Engine, phrases};

pub const GREETING: &str = "Привет! Я мост к нейросетям. Задай любой вопрос или скажи: помощь.";
pub const REFUSAL: &str = "Извини, это семейный навык. Я отвечаю только своим.";

/// Shared server state; `Engine` is cheap to clone (an `Arc` inside).
#[derive(Clone)]
pub struct AppState {
    pub engine: Engine,
    pub webhook_secret: String,
    pub allowed_user_ids: HashSet<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/alice/webhook/{secret}", post(webhook))
        .route("/health", get(|| async { "ok" }))
        .with_state(state)
}

async fn webhook(
    State(state): State<AppState>,
    Path(secret): Path<String>,
    Json(request): Json<WebhookRequest>,
) -> Response {
    if secret != state.webhook_secret {
        return StatusCode::NOT_FOUND.into_response();
    }

    let user_id = request
        .session
        .user
        .as_ref()
        .map(|u| u.user_id.clone())
        .unwrap_or_else(|| request.session.application.application_id.clone());

    if !state.allowed_user_ids.is_empty() && !state.allowed_user_ids.contains(&user_id) {
        tracing::warn!(%user_id, "rejected unknown user");
        return Json(WebhookResponse::say_and_close(REFUSAL)).into_response();
    }

    let utterance = request.request.original_utterance.trim().to_string();
    if request.session.new && utterance.is_empty() {
        return Json(WebhookResponse::say(GREETING)).into_response();
    }

    // A panic inside the engine must still produce a valid Alice response.
    let engine = state.engine.clone();
    let reply = tokio::spawn(async move { engine.handle(&user_id, &utterance).await })
        .await
        .unwrap_or_else(|err| {
            tracing::error!(error = %err, "engine task failed");
            phrases::PHRASE_INTERNAL_ERROR.to_string()
        });
    Json(WebhookResponse::say(reply)).into_response()
}
