use super::{config::ComfyUiConfig, qwen3::Qwen3TtsConfig};
use crate::db::contexts::ContextSettings;
use serde_json::json;
use std::path::Path;

#[test]
fn comfyui_mixed_mode_is_shared_optional_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Database::new(dir.path()).unwrap();
    let ctx = db
        .merge_context(
            "mixed-test",
            json!({
                "settings.comfyui_tts_language_mode": "de-ja",
                "settings.comfyui_xtts_language": "de",
                "settings.comfyui_xtts_reference_audio": "xtts.wav",
                "settings.comfyui_qwen_language": "auto",
                "settings.comfyui_qwen_reference_audio": "qwen.wav",
                "settings.comfyui_qwen_reference_text": "Reference transcript",
                "settings.qwen_tts_language": "French"
            }),
        )
        .unwrap();
    let xtts = ComfyUiConfig::from_settings(&ctx.settings, None).unwrap();
    let qwen = Qwen3TtsConfig::from_settings(&ctx.settings, None).unwrap();
    assert_eq!(xtts.language, "de-ja");
    assert_eq!(qwen.language, "de-ja");
    assert_eq!(xtts.reference_audio, "xtts.wav");
    assert_eq!(qwen.reference_audio, "qwen.wav");
    assert_eq!(qwen.reference_text, "Reference transcript");
    let loaded = db.load_context("mixed-test").unwrap();
    assert_eq!(
        loaded.settings.comfyui_tts_language_mode.as_deref(),
        Some("de-ja")
    );
    assert_eq!(loaded.settings.qwen_tts_language.as_deref(), Some("French"));
    let single = db
        .merge_context(
            "mixed-test",
            json!({"settings.comfyui_tts_language_mode": "single"}),
        )
        .unwrap();
    assert_eq!(
        ComfyUiConfig::from_settings(&single.settings, None)
            .unwrap()
            .language,
        "de"
    );
    assert_eq!(
        Qwen3TtsConfig::from_settings(&single.settings, None)
            .unwrap()
            .language,
        "auto"
    );
}

#[test]
fn comfyui_qwen_settings_precedence_defaults_and_invalid_mode() {
    let settings: ContextSettings = serde_json::from_value(json!({
        "comfyui_base_url": "http://127.0.0.1:8188",
        "comfyui_tts_workflow": "typed.json",
        "comfyui_tts_language_mode": "single",
        "comfyui_qwen_reference_audio": "typed.wav",
        "comfyui_qwen_reference_text": "typed transcript",
        "comfyui_qwen_language": "ja",
        "comfyui_timeout_seconds": 20
    }))
    .unwrap();
    let legacy = json!({"comfyui_qwen_language": "de", "comfyui_qwen_reference_audio": "legacy.wav",
        "comfyui_tts_language_mode": "de-ja", "comfyui_xtts_language": false});
    let typed = Qwen3TtsConfig::resolve(&settings, Some(&legacy), |_| {
        panic!("overridden environment must not be read")
    })
    .unwrap();
    assert_eq!(typed.language, "ja");
    assert_eq!(typed.reference_audio, "typed.wav");
    assert_eq!(typed.reference_text, "typed transcript");
    assert_eq!(typed.timeout.as_secs(), 20);
    let inherited = Qwen3TtsConfig::resolve(&Default::default(), Some(&legacy), |_| None).unwrap();
    assert_eq!(inherited.language, "de-ja");
    assert_eq!(inherited.reference_audio, "legacy.wav");
    let env = Qwen3TtsConfig::resolve(&Default::default(), None, |key| match key {
        "COMFYUI_QWEN_LANGUAGE" => Some("fr".into()),
        "COMFYUI_TTS_LANGUAGE_MODE" => Some("single".into()),
        _ => None,
    })
    .unwrap();
    assert_eq!(env.language, "fr");
    assert_eq!(env.tts_workflow, Path::new("workflows/tts-qwen3-api.json"));
    let defaults = Qwen3TtsConfig::resolve(&Default::default(), None, |_| None).unwrap();
    assert_eq!(defaults.language, "auto");
    assert_eq!(defaults.timeout.as_secs(), 900);
    assert!(
        Qwen3TtsConfig::resolve(&Default::default(), None, |key| (key
            == "COMFYUI_TTS_LANGUAGE_MODE")
            .then(|| "unsupported".into()))
        .is_err()
    );
    for invalid in [
        json!({"comfyui_tts_language_mode":true}),
        json!({"comfyui_qwen_reference_text":42}),
    ] {
        assert!(serde_json::from_value::<ContextSettings>(invalid).is_err());
    }
}

#[tokio::test]
async fn comfyui_qwen_workflow_preserves_bilingual_text_and_server_reference() {
    let settings: ContextSettings = serde_json::from_value(json!({
        "comfyui_tts_workflow": Path::new(env!("CARGO_MANIFEST_DIR")).join("workflows/tts-qwen3-api.json"),
        "comfyui_tts_language_mode": "de-ja",
        "comfyui_qwen_reference_audio": "marvinstimme.wav"
    })).unwrap();
    let config = Qwen3TtsConfig::from_settings(&settings, None).unwrap();
    let text = "Das heißt 学校. Bitte sage がっこう.";
    let graph = super::qwen3::workflow(&config, text).await.unwrap();
    assert_eq!(graph["1"]["class_type"], "PraxisQwen3TTS");
    assert_eq!(
        graph["1"]["inputs"],
        json!({
            "text": text, "language": "de-ja", "reference_audio": "marvinstimme.wav", "reference_text": ""
        })
    );
    assert_eq!(graph.as_object().unwrap().len(), 1);
}

#[tokio::test]
async fn comfyui_xtts_workflow_accepts_shared_mixed_mode() {
    let settings: ContextSettings = serde_json::from_value(json!({
        "comfyui_tts_workflow": Path::new(env!("CARGO_MANIFEST_DIR")).join("workflows/tts-api.json"),
        "comfyui_tts_language_mode": "de-ja",
        "comfyui_xtts_language": "de"
    })).unwrap();
    let config = ComfyUiConfig::from_settings(&settings, None).unwrap();
    let graph = super::workflows::xtts(&config, "Deutsch 日本語 Deutsch")
        .await
        .unwrap();
    assert_eq!(graph["1"]["inputs"]["language"], "de-ja");
    assert_eq!(graph["1"]["class_type"], "PraxisXTTS");
}

#[tokio::test]
async fn comfyui_qwen_rejects_wrong_workflow_paths_language_and_empty_text() {
    let settings: ContextSettings = serde_json::from_value(json!({
        "comfyui_tts_workflow": Path::new(env!("CARGO_MANIFEST_DIR")).join("workflows/tts-qwen3-api.json")
    })).unwrap();
    let mut config = Qwen3TtsConfig::from_settings(&settings, None).unwrap();
    for text in ["".to_string(), "x".repeat(5001)] {
        assert!(super::qwen3::workflow(&config, &text).await.is_err());
    }
    config.reference_audio = "../private.wav".into();
    assert!(super::qwen3::workflow(&config, "Hallo").await.is_err());
    config.reference_audio = "reference.wav".into();
    config.language = "made-up".into();
    assert!(super::qwen3::workflow(&config, "Hallo").await.is_err());
    config.language = "auto".into();
    let ui = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(ui.path(), r#"{"nodes":[],"version":0.4}"#).unwrap();
    config.tts_workflow = ui.path().into();
    let error = super::qwen3::workflow(&config, "Hallo").await.unwrap_err();
    assert!(error.to_string().contains("API-format"));
    assert!(error.to_string().contains(&ui.path().display().to_string()));
    config.tts_workflow = Path::new(env!("CARGO_MANIFEST_DIR")).join("workflows/tts-api.json");
    assert!(super::qwen3::workflow(&config, "Hallo").await.is_err());
}
