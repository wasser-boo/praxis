use super::*;
use base64::Engine;
use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};

#[test]
fn audio_mime_matches_provider_output_instead_of_always_mpeg() {
    assert_eq!(tts_audio_mime(b"RIFFxxxxWAVEdata"), "audio/wav");
    assert_eq!(tts_audio_mime(b"OggSfixture"), "audio/ogg");
    assert_eq!(tts_audio_mime(b"fLaCfixture"), "audio/flac");
    assert_eq!(tts_audio_mime(b"ID3fixture"), "audio/mpeg");
    assert_eq!(tts_audio_mime(b"RIFF"), "audio/mpeg"); // no out-of-bounds reads
}

#[tokio::test]
async fn audio_tts_is_saved_without_subscribers_and_notifications_reference_saved_bytes() {
    // Local synthetic Qwen endpoint only. No real model, credentials or TTS costs.
    let server = MockServer::start().await;
    let audio = b"RIFFxxxxWAVEsynthetic";
    Mock::given(method("POST")).and(path("/tts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "audio": base64::engine::general_purpose::STANDARD.encode(audio),
        })))
        .expect(2).mount(&server).await;
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Database::new(dir.path()).unwrap();
    let user = format!("audio-test-{}", uuid::Uuid::new_v4());
    db.save_context(&db.load_context(&user).unwrap()).unwrap();
    let settings = crate::db::contexts::ContextSettings {
        voice_tts_type: "qwen_tts".into(),
        qwen_tts_server: Some(server.uri()),
        web_chat_tts: true,
        ..Default::default()
    };
    for subscribed in [false, true] {
        let mut rx = subscribed.then(|| crate::dashboard::stream::get_or_create(&user).subscribe());
        let id = db.add_message(&user, &crate::db::messages::Message::assistant("fixture reply".into())).unwrap();
        spawn_tts("fixture reply".into(), &settings, &Default::default(), &user, &db, Some(id));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while db.get_message_audio(&user, id).unwrap().is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.unwrap();
        assert_eq!(db.get_message_audio(&user, id).unwrap(), Some(("audio/wav".into(), audio.to_vec())));
        if let Some(rx) = rx.as_mut() {
            let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await.unwrap().unwrap();
            assert_eq!(event.event, "chat_tts");
            let payload: serde_json::Value = serde_json::from_str(&event.data).unwrap();
            assert_eq!(payload["message_id"], id);
            assert_eq!(payload["mime"], "audio/wav");
            assert!(payload.get("audio").is_none(), "SSE should not buffer huge base64 blobs for saved replies");
        }
    }
    crate::dashboard::stream::remove(&user);
}
