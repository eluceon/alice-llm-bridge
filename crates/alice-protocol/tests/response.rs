use alice_protocol::{MAX_TEXT_LEN, WebhookResponse, clip_to_limit};

#[test]
fn serializes_expected_shape() {
    let resp = WebhookResponse::say("Привет!");
    let value = serde_json::to_value(&resp).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "response": { "text": "Привет!", "tts": "Привет!", "end_session": false },
            "version": "1.0"
        })
    );
}

#[test]
fn say_and_close_ends_session() {
    let resp = WebhookResponse::say_and_close("Пока");
    assert!(resp.response.end_session);
}

#[test]
fn short_text_is_untouched() {
    assert_eq!(
        clip_to_limit("Марс — четвёртая планета."),
        "Марс — четвёртая планета."
    );
}

#[test]
fn long_text_is_clipped_at_sentence_boundary() {
    let sentence = "Это довольно длинное предложение о космосе и планетах. ";
    let long: String = sentence.repeat(40);
    let clipped = clip_to_limit(&long);
    assert!(clipped.chars().count() <= MAX_TEXT_LEN);
    assert!(clipped.ends_with('.'));
}

#[test]
fn text_without_sentences_is_clipped_at_whitespace() {
    let long = "слово ".repeat(300);
    let clipped = clip_to_limit(&long);
    assert!(clipped.chars().count() <= MAX_TEXT_LEN);
    assert!(clipped.ends_with('…'));
}
