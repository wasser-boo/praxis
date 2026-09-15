use super::*;
use serde_json::json;
use wiremock::{
    matchers::{body_json, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn comfyui_xtts_reuses_audio_persistence_and_discord_event_with_per_user_settings() {
    let server = MockServer::start().await;
    let audio = crate::voice::pcm_to_wav(&[1000, -1000, 0], 24000, 1);
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Database::new(dir.path()).unwrap();
    crate::event_channel::init();
    let mut events = crate::event_channel::get_event_tx().unwrap().subscribe();
    for (language, reference) in [("de", "voices/alice.wav"), ("fr", "voices/bob.wav")] {
        let user = format!("comfyui-tts-{}", uuid::Uuid::new_v4());
        Mock::given(method("POST"))
            .and(path("/prompt"))
            .and(body_json(
                json!({"prompt": {"1": {"class_type": "PraxisXTTS", "inputs": {
                    "text": "fixture reply", "language": language, "reference_audio": reference
                }}}}),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"prompt_id": language, "node_errors": {}})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET")).and(path(format!("/history/{language}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({(language): {
                "status": {"completed": true, "status_str": "success"},
                "outputs": {"1": {"audio": [{"filename": format!("{language}.wav"), "subfolder": "", "type": "output"}]}}
            }}))).expect(1).mount(&server).await;
        Mock::given(method("GET"))
            .and(path("/view"))
            .and(query_param("filename", format!("{language}.wav")))
            .and(query_param("subfolder", ""))
            .and(query_param("type", "output"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(audio.as_slice()))
            .expect(1)
            .mount(&server)
            .await;
        let ctx = db.merge_context(&user, json!({
            "settings.voice_tts_type": "comfyui_xtts", "settings.use_tts": true,
            "settings.web_chat_tts": false, "settings.voice_elevenlabs_voice_id": "preserve-me",
            "settings.qwen_tts_language": "English",
            "settings.comfyui_base_url": server.uri(),
            "settings.comfyui_tts_workflow": std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("workflows/tts-api.json"),
            "settings.comfyui_xtts_language": language,
            "settings.comfyui_xtts_reference_audio": reference,
            "settings.comfyui_timeout_seconds": 3,
            // Typed settings win over previously saved extension values.
            "custom_data.comfyui_base_url": "http://127.0.0.1:1",
            "custom_data.comfyui_tts_workflow": "missing-legacy-workflow.json",
            "custom_data.comfyui_xtts_language": "en",
            "custom_data.comfyui_xtts_reference_audio": "legacy.wav",
            "custom_data.comfyui_timeout_seconds": 0,
        })).unwrap();
        let id = db
            .add_message(
                &user,
                &crate::db::messages::Message::assistant("fixture reply".into()),
            )
            .unwrap();
        let guard = crate::gateway::task_control::begin(&user).unwrap();
        spawn_tts(
            "fixture reply".into(),
            &ctx.settings,
            &Default::default(),
            &user,
            &db,
            Some(id),
            Some("voice:123"),
        );
        // Final speech must survive normal conversation-task completion.
        drop(guard);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let crate::event_channel::GatewayEvent::VoiceTts {
                    user_id,
                    audio_data,
                } = events.recv().await.unwrap()
                {
                    if user_id == user {
                        assert_eq!(audio_data, audio);
                        break;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            db.get_message_audio(&user, id).unwrap(),
            Some(("audio/wav".into(), audio.clone()))
        );
        let preserved = db.load_context(&user).unwrap();
        assert_eq!(preserved.settings.comfyui_base_url, Some(server.uri()));
        assert_eq!(
            preserved.settings.comfyui_xtts_language.as_deref(),
            Some(language)
        );
        assert_eq!(
            preserved.settings.comfyui_xtts_reference_audio.as_deref(),
            Some(reference)
        );
        assert_eq!(preserved.settings.comfyui_timeout_seconds, Some(3));
        assert_eq!(
            preserved.settings.voice_elevenlabs_voice_id.as_deref(),
            Some("preserve-me")
        );
        assert_eq!(
            preserved.settings.qwen_tts_language.as_deref(),
            Some("English")
        );
    }
}
