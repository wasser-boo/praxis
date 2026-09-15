//! Per-user settings -> legacy custom_data -> process environment -> defaults.
use crate::db::contexts::ContextSettings;
use anyhow::{ensure, Context as _};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug)]
pub struct ComfyUiConfig {
    pub base_url: String,
    pub tts_workflow: PathBuf,
    pub reference_audio: String,
    pub language: String,
    pub timeout: Duration,
}

impl ComfyUiConfig {
    pub fn from_settings(
        settings: &ContextSettings,
        legacy: Option<&Value>,
    ) -> anyhow::Result<Self> {
        Self::resolve(settings, legacy, |name| std::env::var(name).ok())
    }

    fn resolve(
        settings: &ContextSettings,
        legacy: Option<&Value>,
        env: impl Fn(&str) -> Option<String>,
    ) -> anyhow::Result<Self> {
        let setting = |typed, key, variable, default| {
            string_setting(typed, legacy, key, variable, default, &env)
        };
        let timeout = timeout_setting(settings, legacy, &env)?;
        Ok(Self {
            base_url: setting(
                settings.comfyui_base_url.as_deref(),
                "comfyui_base_url",
                "COMFYUI_BASE_URL",
                "",
            )?,
            tts_workflow: setting(
                settings.comfyui_tts_workflow.as_deref(),
                "comfyui_tts_workflow",
                "COMFYUI_TTS_WORKFLOW",
                "workflows/tts-api.json",
            )?
            .into(),
            reference_audio: setting(
                settings.comfyui_xtts_reference_audio.as_deref(),
                "comfyui_xtts_reference_audio",
                "COMFYUI_XTTS_REFERENCE_AUDIO",
                "reference.wav",
            )?,
            language: effective_language(settings, legacy, &env, || {
                setting(
                    settings.comfyui_xtts_language.as_deref(),
                    "comfyui_xtts_language",
                    "COMFYUI_XTTS_LANGUAGE",
                    "en",
                )
            })?,
            timeout,
        })
    }
}

pub(super) fn string_setting(
    typed: Option<&str>,
    legacy: Option<&Value>,
    key: &str,
    variable: &str,
    default: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> anyhow::Result<String> {
    if let Some(value) = typed.filter(|v| !v.is_empty()) {
        return Ok(value.to_string());
    }
    if let Some(value) = legacy.and_then(|ctx| ctx.get(key)).filter(|v| !v.is_null()) {
        let value = value
            .as_str()
            .with_context(|| format!("Legacy custom_data.{key} must be a string"))?;
        if !value.is_empty() {
            return Ok(value.to_string());
        }
    }
    Ok(env(variable)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string()))
}

pub(super) fn timeout_setting(
    settings: &ContextSettings,
    legacy: Option<&Value>,
    env: &impl Fn(&str) -> Option<String>,
) -> anyhow::Result<Duration> {
    let seconds = if let Some(seconds) = settings.comfyui_timeout_seconds {
        seconds as u64
    } else if let Some(value) = legacy
        .and_then(|v| v.get("comfyui_timeout_seconds"))
        .filter(|v| !v.is_null())
    {
        value
            .as_u64()
            .context("Legacy custom_data.comfyui_timeout_seconds must be a positive integer")?
    } else {
        env("COMFYUI_TIMEOUT_SECONDS")
            .unwrap_or_else(|| "900".into())
            .parse::<u64>()
            .context("COMFYUI_TIMEOUT_SECONDS must be a positive integer")?
    };
    ensure!(
        (1..=3600).contains(&seconds),
        "ComfyUI timeout must be 1–3600 seconds"
    );
    Ok(Duration::from_secs(seconds))
}

pub(super) fn effective_language(
    settings: &ContextSettings,
    legacy: Option<&Value>,
    env: &impl Fn(&str) -> Option<String>,
    single: impl FnOnce() -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    let mode = string_setting(
        settings.comfyui_tts_language_mode.as_deref(),
        legacy,
        "comfyui_tts_language_mode",
        "COMFYUI_TTS_LANGUAGE_MODE",
        "single",
        env,
    )?;
    match mode.as_str() {
        "single" => single(),
        "de-ja" => Ok(mode),
        _ => anyhow::bail!("ComfyUI speech language mode must be single or de-ja"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn comfyui_settings_override_legacy_and_environment_per_field() {
        let settings = ContextSettings {
            comfyui_base_url: Some("http://100.80.1.2:8188".into()),
            comfyui_tts_workflow: Some("/local/tts.json".into()),
            comfyui_xtts_reference_audio: Some("voices/person.wav".into()),
            comfyui_xtts_language: Some("de".into()),
            comfyui_tts_language_mode: Some("single".into()),
            comfyui_timeout_seconds: Some(120),
            qwen_tts_language: Some("French".into()),
            ..Default::default()
        };
        // Even malformed legacy values are irrelevant when settings override.
        let legacy = json!({"comfyui_base_url": false, "comfyui_tts_workflow": [],
            "comfyui_xtts_reference_audio": 42, "comfyui_xtts_language": "es", "comfyui_timeout_seconds": 0});
        let config = ComfyUiConfig::resolve(&settings, Some(&legacy), |_| {
            panic!("must not read environment for overridden fields")
        })
        .unwrap();
        assert_eq!(config.base_url, "http://100.80.1.2:8188");
        assert_eq!(config.tts_workflow, PathBuf::from("/local/tts.json"));
        assert_eq!(config.reference_audio, "voices/person.wav");
        assert_eq!(config.language, "de");
        assert_eq!(config.timeout, Duration::from_secs(120));
        assert_eq!(settings.qwen_tts_language.as_deref(), Some("French"));
    }

    #[test]
    fn comfyui_legacy_configuration_and_environment_fallback_remain_supported() {
        let settings: ContextSettings = serde_json::from_str("{}").unwrap();
        assert!(settings.comfyui_base_url.is_none());
        assert!(settings.comfyui_tts_workflow.is_none());
        assert!(settings.comfyui_xtts_reference_audio.is_none());
        assert!(settings.comfyui_xtts_language.is_none());
        assert!(settings.comfyui_timeout_seconds.is_none());
        let legacy = json!({"comfyui_xtts_language": "de", "comfyui_xtts_reference_audio": "voices/person.wav",
            "comfyui_timeout_seconds": 120});
        let config = ComfyUiConfig::resolve(&settings, Some(&legacy), |name| match name {
            "COMFYUI_XTTS_LANGUAGE" => Some("es".into()),
            "COMFYUI_BASE_URL" => Some("http://100.80.1.2:8188".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(config.language, "de");
        assert_eq!(config.reference_audio, "voices/person.wav");
        assert_eq!(config.base_url, "http://100.80.1.2:8188");
        assert_eq!(config.timeout, Duration::from_secs(120));
        let defaults = ComfyUiConfig::resolve(&settings, None, |_| None).unwrap();
        assert_eq!(defaults.language, "en");
        assert_eq!(defaults.timeout, Duration::from_secs(900));
        assert_eq!(
            defaults.tts_workflow,
            PathBuf::from("workflows/tts-api.json")
        );
        assert_eq!(defaults.reference_audio, "reference.wav");
    }

    #[test]
    fn comfyui_empty_settings_inherit_without_hiding_invalid_timeouts() {
        let settings = ContextSettings {
            comfyui_base_url: Some(String::new()),
            comfyui_xtts_language: Some(String::new()),
            ..Default::default()
        };
        let legacy = json!({"comfyui_base_url": "", "comfyui_xtts_language": "fr"});
        let config = ComfyUiConfig::resolve(&settings, Some(&legacy), |name| match name {
            "COMFYUI_BASE_URL" => Some("http://100.80.1.2:8188".into()),
            "COMFYUI_TIMEOUT_SECONDS" => Some("300".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(config.base_url, "http://100.80.1.2:8188");
        assert_eq!(config.language, "fr");
        assert_eq!(config.timeout, Duration::from_secs(300));
        for seconds in [0, 3601] {
            let invalid = ContextSettings {
                comfyui_timeout_seconds: Some(seconds),
                ..Default::default()
            };
            assert!(ComfyUiConfig::resolve(
                &invalid,
                Some(&json!({"comfyui_timeout_seconds": 120})),
                |_| None
            )
            .is_err());
        }
        for legacy in [
            json!({"comfyui_timeout_seconds": 1.5}),
            json!({"comfyui_xtts_language": false}),
        ] {
            assert!(ComfyUiConfig::resolve(&settings, Some(&legacy), |_| None).is_err());
        }
        assert!(ComfyUiConfig::resolve(&settings, Some(&json!([])), |_| None).is_ok());
    }

    #[test]
    fn comfyui_settings_survive_context_save_reload_and_validate_json_types() {
        let directory = tempfile::tempdir().unwrap();
        let db = crate::db::Database::new(directory.path()).unwrap();
        let saved = db
            .merge_context(
                "comfyui-settings",
                json!({
                    "settings.comfyui_base_url": "http://100.80.1.2:8188",
                    "settings.comfyui_tts_workflow": "workflows/tts-api.json",
                    "settings.comfyui_xtts_reference_audio": "voices/person.wav",
                    "settings.comfyui_xtts_language": "de",
                    "settings.comfyui_timeout_seconds": 300,
                }),
            )
            .unwrap();
        drop(db);
        let db = crate::db::Database::new(directory.path()).unwrap();
        let loaded = db.load_context("comfyui-settings").unwrap();
        assert_eq!(
            serde_json::to_value(&loaded.settings).unwrap(),
            serde_json::to_value(&saved.settings).unwrap()
        );
        let config = ComfyUiConfig::resolve(&loaded.settings, None, |_| None).unwrap();
        assert_eq!(config.language, "de");
        assert_eq!(config.timeout, Duration::from_secs(300));
        for invalid in [
            json!({"comfyui_xtts_language": false}),
            json!({"comfyui_timeout_seconds": -1}),
            json!({"comfyui_timeout_seconds": 1.5}),
        ] {
            assert!(serde_json::from_value::<ContextSettings>(invalid).is_err());
        }
    }
}
