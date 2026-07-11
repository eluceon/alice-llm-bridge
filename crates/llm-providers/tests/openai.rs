use std::time::Duration;

use llm_providers::{ChatMessage, ChatProvider, ChatRequest, OpenAiCompatClient, ProviderError};
use wiremock::matchers::{bearer_token, body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn request() -> ChatRequest {
    ChatRequest {
        model: "deepseek-chat".to_string(),
        messages: vec![
            ChatMessage::system("Ты голосовой помощник."),
            ChatMessage::user("Привет"),
        ],
        max_tokens: 300,
        temperature: 0.7,
    }
}

fn client(server: &MockServer) -> OpenAiCompatClient {
    OpenAiCompatClient::new(
        &server.uri(),
        "test-key".to_string(),
        Duration::from_secs(2),
    )
    .unwrap()
}

fn success_body() -> serde_json::Value {
    serde_json::json!({
        "id": "chatcmpl-1",
        "choices": [
            {
                "index": 0,
                "message": { "role": "assistant", "content": "Привет! Чем помочь?" },
                "finish_reason": "stop"
            }
        ],
        "usage": { "prompt_tokens": 25, "completion_tokens": 9, "total_tokens": 34 }
    })
}

#[tokio::test]
async fn parses_successful_completion() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(bearer_token("test-key"))
        .and(body_partial_json(serde_json::json!({
            "model": "deepseek-chat",
            "max_tokens": 300
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .mount(&server)
        .await;

    let completion = client(&server).chat(&request()).await.unwrap();
    assert_eq!(completion.text, "Привет! Чем помочь?");
    assert_eq!(completion.usage.prompt_tokens, 25);
    assert_eq!(completion.usage.completion_tokens, 9);
}

#[tokio::test]
async fn classifies_rate_limit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;

    let err = client(&server).chat(&request()).await.unwrap_err();
    assert!(matches!(err, ProviderError::RateLimited));
}

#[tokio::test]
async fn classifies_auth_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let err = client(&server).chat(&request()).await.unwrap_err();
    assert!(matches!(err, ProviderError::Auth));
}

#[tokio::test]
async fn retries_once_on_server_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(success_body()))
        .expect(1)
        .mount(&server)
        .await;

    let completion = client(&server).chat(&request()).await.unwrap();
    assert_eq!(completion.text, "Привет! Чем помочь?");
}

#[tokio::test]
async fn maps_slow_response_to_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(success_body())
                .set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;

    let fast_client =
        OpenAiCompatClient::new(&server.uri(), "k".to_string(), Duration::from_millis(100))
            .unwrap();
    let err = fast_client.chat(&request()).await.unwrap_err();
    assert!(matches!(err, ProviderError::Timeout));
}

#[tokio::test]
async fn rejects_response_without_choices() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"choices": []})))
        .mount(&server)
        .await;

    let err = client(&server).chat(&request()).await.unwrap_err();
    assert!(matches!(err, ProviderError::InvalidResponse(_)));
}
