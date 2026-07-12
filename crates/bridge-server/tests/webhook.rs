use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use bridge_core::testing::{MemoryStore, ScriptedProvider};
use bridge_core::{
    Engine, EngineConfig, FamilyRoster, ModelPreset, ModelRegistry, Profile, ProfileRole, phrases,
};
use bridge_server::routes::{AppState, GREETING, router};
use tower::util::ServiceExt;

fn engine(provider: Arc<ScriptedProvider>) -> Engine {
    let preset = ModelPreset {
        provider,
        model: "test-model".to_string(),
        max_tokens: 300,
        temperature: 0.7,
        input_price_per_mtok: 1.0,
        output_price_per_mtok: 2.0,
    };
    let roster = FamilyRoster::new(
        vec![Profile {
            name: "Дима".to_string(),
            aliases: vec!["дима".to_string()],
            birthday: None,
            role: ProfileRole::Adult,
            persona: String::new(),
        }],
        "Дима",
    )
    .unwrap();
    Engine::new(
        roster,
        Vec::new(),
        ModelRegistry {
            fast: preset.clone(),
            smart: preset,
        },
        Arc::new(MemoryStore::new()),
        EngineConfig {
            context_window: 4,
            reply_budget: Duration::from_millis(200),
            utc_offset_hours: 3,
        },
    )
}

fn state(provider: Arc<ScriptedProvider>) -> AppState {
    AppState {
        engine: engine(provider),
        webhook_secret: "s3cret".to_string(),
        allowed_user_ids: HashSet::from(["ALLOWED".to_string()]),
    }
}

fn alice_request(user_id: &str, utterance: &str, new_session: bool) -> serde_json::Value {
    serde_json::json!({
        "meta": { "locale": "ru-RU", "timezone": "Europe/Moscow" },
        "session": {
            "message_id": 1,
            "session_id": "sess",
            "skill_id": "skill",
            "user": { "user_id": user_id },
            "application": { "application_id": "app" },
            "new": new_session
        },
        "request": {
            "command": utterance.to_lowercase(),
            "original_utterance": utterance,
            "type": "SimpleUtterance"
        },
        "version": "1.0"
    })
}

async fn post(
    app: axum::Router,
    secret: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/alice/webhook/{secret}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

#[tokio::test]
async fn wrong_secret_is_not_found() {
    let app = router(state(ScriptedProvider::replying("ок")));
    let (status, _) = post(app, "wrong", alice_request("ALLOWED", "привет", false)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_user_is_refused() {
    let app = router(state(ScriptedProvider::replying("ок")));
    let (status, body) = post(app, "s3cret", alice_request("STRANGER", "привет", false)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["response"]["end_session"], serde_json::json!(true));
}

#[tokio::test]
async fn new_session_gets_greeting() {
    let app = router(state(ScriptedProvider::replying("ок")));
    let (_, body) = post(app, "s3cret", alice_request("ALLOWED", "", true)).await;
    assert_eq!(body["response"]["text"], serde_json::json!(GREETING));
    assert_eq!(body["response"]["end_session"], serde_json::json!(false));
}

#[tokio::test]
async fn question_is_answered() {
    let app = router(state(ScriptedProvider::replying("Марс — планета.")));
    let (_, body) = post(
        app,
        "s3cret",
        alice_request("ALLOWED", "Что такое Марс?", false),
    )
    .await;
    assert_eq!(
        body["response"]["text"],
        serde_json::json!("Марс — планета.")
    );
    assert_eq!(body["version"], serde_json::json!("1.0"));
}

#[tokio::test(start_paused = true)]
async fn slow_answer_is_deferred_across_requests() {
    let st = state(ScriptedProvider::slow(
        "готовый ответ",
        Duration::from_secs(5),
    ));

    let (_, first) = post(
        router(st.clone()),
        "s3cret",
        alice_request("ALLOWED", "сложный вопрос", false),
    )
    .await;
    assert_eq!(
        first["response"]["text"],
        serde_json::json!(phrases::PHRASE_THINKING_STARTED)
    );

    tokio::time::sleep(Duration::from_secs(6)).await;

    let (_, second) = post(
        router(st),
        "s3cret",
        alice_request("ALLOWED", "ну что", false),
    )
    .await;
    assert_eq!(
        second["response"]["text"],
        serde_json::json!("готовый ответ")
    );
}

#[tokio::test]
async fn health_endpoint_responds() {
    let app = router(state(ScriptedProvider::replying("ок")));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
