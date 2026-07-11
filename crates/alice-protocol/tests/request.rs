use alice_protocol::{RequestKind, WebhookRequest};

const SAMPLE: &str = r#"{
  "meta": {
    "locale": "ru-RU",
    "timezone": "Europe/Moscow",
    "client_id": "ru.yandex.searchplugin/7.16 (none none; android 4.4.2)",
    "interfaces": { "screen": {}, "account_linking": {} }
  },
  "session": {
    "message_id": 3,
    "session_id": "2eac4854-fce7-40b3-8b58-c1c8d8f52ea3",
    "skill_id": "3ad36498-f5rd-4079-a14b-788652932056",
    "user": { "user_id": "6C91DA5198D1758C6A9F63A7C5CDDF09" },
    "application": { "application_id": "47C73714B580ED2469056E71081159529FFC676A4E5B059D629A819E857DC2F8" },
    "new": false
  },
  "request": {
    "command": "расскажи про марс",
    "original_utterance": "Расскажи про Марс",
    "type": "SimpleUtterance",
    "markup": { "dangerous_context": false },
    "nlu": { "tokens": ["расскажи", "про", "марс"], "entities": [], "intents": {} }
  },
  "version": "1.0"
}"#;

#[test]
fn deserializes_real_webhook_request() {
    let req: WebhookRequest = serde_json::from_str(SAMPLE).unwrap();
    assert_eq!(req.request.command, "расскажи про марс");
    assert_eq!(req.request.original_utterance, "Расскажи про Марс");
    assert_eq!(req.request.kind, RequestKind::SimpleUtterance);
    assert!(!req.session.new);
    assert_eq!(
        req.session.user.as_ref().unwrap().user_id,
        "6C91DA5198D1758C6A9F63A7C5CDDF09"
    );
    assert_eq!(req.meta.timezone, "Europe/Moscow");
    assert_eq!(req.version, "1.0");
}

#[test]
fn tolerates_missing_user_and_unknown_request_type() {
    let json = r#"{
      "meta": { "locale": "ru-RU", "timezone": "UTC" },
      "session": {
        "message_id": 0,
        "session_id": "s",
        "skill_id": "sk",
        "application": { "application_id": "app-1" },
        "new": true
      },
      "request": { "type": "AudioPlayer.PlaybackStarted" },
      "version": "1.0"
    }"#;
    let req: WebhookRequest = serde_json::from_str(json).unwrap();
    assert!(req.session.user.is_none());
    assert_eq!(req.request.kind, RequestKind::Other);
    assert_eq!(req.request.command, "");
}
