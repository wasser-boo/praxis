use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use dashmap::DashMap;

pub struct VoiceState {
    pub guild_id: u64,
    pub channel_id: u64,
    pub user_id: String,
    pub speaking: bool,
}

pub struct VoiceHandler {
    pub states: Arc<DashMap<u64, VoiceState>>,
    pub muted: Arc<Mutex<bool>>,
    pub audio_buffer: Arc<Mutex<Vec<i16>>>,
    pub last_speech_time: Arc<Mutex<std::time::Instant>>,
    pub last_active_ssrc: Arc<Mutex<Option<u32>>>,
    pub sample_rate: u32,
    pub ssrc_to_user: Arc<DashMap<u32, u64>>,
    pub transcription_tx: Arc<Mutex<Option<mpsc::Sender<(String, Vec<i16>)>>>>,
    pub user_buffers: Arc<DashMap<u32, Vec<i16>>>,
    pub user_last_speech: Arc<DashMap<u32, std::time::Instant>>,
    pub fallback_user_id: Arc<Mutex<Option<String>>>,
    pub allowed_discord_ids: Arc<Mutex<std::collections::HashSet<u64>>>,
}

impl VoiceHandler {
    pub fn new() -> Self {
        Self {
            states: Arc::new(DashMap::new()),
            muted: Arc::new(Mutex::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            last_speech_time: Arc::new(Mutex::new(std::time::Instant::now())),
            last_active_ssrc: Arc::new(Mutex::new(None)),
            sample_rate: 16000,
            ssrc_to_user: Arc::new(DashMap::new()),
            transcription_tx: Arc::new(Mutex::new(None)),
            user_buffers: Arc::new(DashMap::new()),
            user_last_speech: Arc::new(DashMap::new()),
            fallback_user_id: Arc::new(Mutex::new(None)),
            allowed_discord_ids: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub async fn set_allowed_discord_ids(&self, ids: Vec<u64>) {
        let mut set = self.allowed_discord_ids.lock().await;
        *set = ids.into_iter().collect();
        tracing::info!("Voice: allowed Discord users: {:?}", set);
    }

    pub async fn is_user_allowed(&self, discord_id: u64) -> bool {
        let set = self.allowed_discord_ids.lock().await;
        set.is_empty() || set.contains(&discord_id)
    }

    pub async fn set_fallback_user(&self, user_id: String) {
        tracing::info!("Voice: fallback user set to {}", user_id);
        *self.fallback_user_id.lock().await = Some(user_id);
    }

    pub async fn set_transcription_channel(&self, tx: mpsc::Sender<(String, Vec<i16>)>) {
        let mut lock = self.transcription_tx.lock().await;
        *lock = Some(tx);
    }

    pub fn set_ssrc_user(&self, ssrc: u32, user_id: u64) {
        self.ssrc_to_user.insert(ssrc, user_id);
        let buffered = self.user_buffers.get(&ssrc).map(|b| b.len()).unwrap_or(0);
        tracing::info!("SSRC {} -> user {} ({} buffered samples pending)", ssrc, user_id, buffered);
    }

    pub fn get_user_from_ssrc(&self, ssrc: u32) -> Option<u64> {
        self.ssrc_to_user.get(&ssrc).map(|r| *r.value())
    }

    pub fn remove_user(&self, user_id: u64) {
        self.ssrc_to_user.retain(|_, &mut v| v != user_id);
        tracing::debug!("Removed user {} from SSRC mapping", user_id);
    }

    pub async fn add_audio_raw(&self, audio: &[i16], ssrc: u32) {
        // If SSRC is mapped to a user, check if they're allowed
        if let Some(discord_id) = self.get_user_from_ssrc(ssrc) {
            if !self.is_user_allowed(discord_id).await {
                return;
            }
        }

        self.user_buffers.entry(ssrc).or_insert_with(Vec::new).extend_from_slice(audio);
        self.user_last_speech.insert(ssrc, std::time::Instant::now());

        let mut buffer = self.audio_buffer.lock().await;
        buffer.extend_from_slice(audio);

        let has_content = audio.iter().any(|&s| s.abs() > 50);
        if has_content {
            let mut last_time = self.last_speech_time.lock().await;
            *last_time = std::time::Instant::now();
            drop(last_time);
            let mut last_ssrc = self.last_active_ssrc.lock().await;
            *last_ssrc = Some(ssrc);
            let user_mapped = self.get_user_from_ssrc(ssrc).is_some();
            tracing::debug!("VOICE: Speech detected (ssrc={}, mapped={}, buffer={} samples)",
                ssrc, user_mapped,
                self.user_buffers.get(&ssrc).map(|b| b.len()).unwrap_or(0));
        }
    }

    pub async fn process_audio(&self, current_ssrc: u32) {
        // If SSRC is mapped to a non-allowed user, skip
        if let Some(discord_id) = self.get_user_from_ssrc(current_ssrc) {
            if !self.is_user_allowed(discord_id).await {
                return;
            }
        }

        // Get user: either from SSRC mapping or fallback
        let user_id = match self.get_user_from_ssrc(current_ssrc) {
            Some(uid) => uid.to_string(),
            None => {
                match self.fallback_user_id.lock().await.clone() {
                    Some(uid) => uid,
                    None => return,
                }
            }
        };

        let time_since_speech = self.time_since_last_speech().await;
        let pause_threshold = std::time::Duration::from_secs_f32(0.5);

        if time_since_speech < pause_threshold {
            return;
        }

        let (buffer_len, audio_data) = if let Some(mut entry) = self.user_buffers.get_mut(&current_ssrc) {
            let len = entry.len();
            let data = entry.clone();
            entry.clear();
            (len, data)
        } else {
            return;
        };

        let min_duration_secs = 0.2;
        let min_samples = (self.sample_rate as f32 * min_duration_secs) as usize;

        if buffer_len < min_samples {
            return;
        }

        tracing::info!("VOICE_HANDLER: Pause detected ({}ms), sending {} samples for transcription (ssrc={}, user={})",
            time_since_speech.as_millis(), buffer_len, current_ssrc, user_id);

        let tx_guard = self.transcription_tx.lock().await;
        if let Some(tx) = tx_guard.as_ref() {
            let _ = tx.send((user_id, audio_data)).await;
        }
    }

    pub async fn set_muted(&self, muted: bool) {
        let mut m = self.muted.lock().await;
        *m = muted;
        tracing::info!("Voice handler muted set to: {}", muted);
    }

    pub async fn is_muted(&self) -> bool {
        *self.muted.lock().await
    }

    pub async fn time_since_last_speech(&self) -> std::time::Duration {
        let last = self.last_speech_time.lock().await;
        last.elapsed()
    }
}

// ── Songbird Integration ─────────────────────────────────────────────────────

#[cfg(feature = "songbird")]
pub mod songbird_integration {
    use super::*;
    use songbird::events::{Event, EventContext, EventHandler as SongbirdEventHandler};
    use songbird::{Config, driver::{DecodeMode, DecodeConfig, Channels, SampleRate}};
    use async_trait::async_trait;

    pub fn create_songbird_config() -> Config {
        Config::default().decode_mode(DecodeMode::Decode(DecodeConfig::new(Channels::Mono, SampleRate::Hz16000)))
    }

    #[derive(Clone)]
    pub struct VoiceReceiver {
        handler: Arc<VoiceHandler>,
    }

    impl VoiceReceiver {
        pub fn new(handler: Arc<VoiceHandler>) -> Self {
            Self { handler }
        }
    }

    #[async_trait]
    impl SongbirdEventHandler for VoiceReceiver {
        async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
            match ctx {
                EventContext::SpeakingStateUpdate(speaking) => {
                    tracing::info!("VOICE_EVENT: SpeakingStateUpdate ssrc={}, user_id={:?}, speaking={:?}", speaking.ssrc, speaking.user_id, speaking.speaking);
                    if let Some(user_id) = speaking.user_id {
                        self.handler.set_ssrc_user(speaking.ssrc, user_id.0);
                    }
                },
                EventContext::VoiceTick(tick) => {
                    let speaking_count = tick.speaking.len();
                    if speaking_count > 0 {
                        let unmapped: Vec<u32> = tick.speaking.keys()
                            .filter(|ssrc| self.handler.get_user_from_ssrc(**ssrc).is_none())
                            .copied()
                            .collect();
                        if !unmapped.is_empty() {
                            tracing::debug!("VOICETICK: {} speaking, unmapped={:?}", speaking_count, unmapped);
                        }
                    }
                    for (ssrc, data) in &tick.speaking {
                        let user_id_str = self.handler.get_user_from_ssrc(*ssrc).map(|u| u.to_string()).unwrap_or_else(|| "?".into());
                        tracing::trace!("VOICETICK: Processing SSRC {}/{}", ssrc, user_id_str);
                        if self.handler.get_user_from_ssrc(*ssrc).is_none() {
                            tracing::debug!("VOICETICK: Unknown SSRC {} - buffering audio, waiting for SpeakingStateUpdate", ssrc);
                        }
                        if let Some(decoded_voice) = &data.decoded_voice {
                            let non_zero = decoded_voice.iter().filter(|&&s| s != 0).count();
                            tracing::trace!("VOICETICK: SSRC {} ({}) - {} samples, {} non-zero", ssrc, user_id_str, decoded_voice.len(), non_zero);
                            if non_zero > 0 {
                                self.handler.add_audio_raw(decoded_voice, *ssrc).await;
                            }
                        } else {
                            tracing::trace!("VOICETICK: SSRC {} ({}) - no decoded voice", ssrc, user_id_str);
                        }
                    }
                    // Process audio for all speaking users and any SSRC with pending buffered audio
                    let mut ssrcs_to_process: Vec<u32> = tick.speaking.keys().copied().collect();
                    // Also check SSRCs that have buffers (might have been buffered before mapping)
                    for entry in self.handler.user_buffers.iter() {
                        let ssrc = *entry.key();
                        if !ssrcs_to_process.contains(&ssrc) && !entry.value().is_empty() {
                            ssrcs_to_process.push(ssrc);
                        }
                    }
                    for ssrc in ssrcs_to_process {
                        let _ = self.handler.process_audio(ssrc).await;
                    }
                },
                EventContext::ClientDisconnect(disc) => {
                    tracing::debug!("VOICE_EVENT: ClientDisconnect user_id={}", disc.user_id);
                    if disc.user_id.0 != 0 {
                        self.handler.remove_user(disc.user_id.0);
                    }
                },
                other => {
                    tracing::debug!("VOICE_EVENT: Other event type received");
                    let _ = other;
                }
            }
            None
        }
    }
}

#[cfg(not(feature = "songbird"))]
pub mod songbird_integration {
    use super::*;

    pub fn create_songbird_config() {
        tracing::warn!("Songbird not enabled - voice features unavailable");
    }

    #[derive(Clone)]
    pub struct VoiceReceiver {
        _handler: Arc<VoiceHandler>,
    }

    impl VoiceReceiver {
        pub fn new(handler: Arc<VoiceHandler>) -> Self {
            Self { _handler: handler }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod handler_tests {
    use super::*;

    #[tokio::test]
    async fn test_voice_handler_new() {
        let handler = VoiceHandler::new();
        assert!(!handler.is_muted().await);
        assert_eq!(handler.sample_rate, 16000);
        assert!(handler.get_user_from_ssrc(1234).is_none());
    }

    #[tokio::test]
    async fn test_ssrc_user_mapping() {
        let handler = VoiceHandler::new();
        handler.set_ssrc_user(100, 42);
        assert_eq!(handler.get_user_from_ssrc(100), Some(42));
        assert_eq!(handler.get_user_from_ssrc(999), None);
    }

    #[tokio::test]
    async fn test_remove_user() {
        let handler = VoiceHandler::new();
        handler.set_ssrc_user(100, 42);
        handler.set_ssrc_user(200, 42);
        handler.remove_user(42);
        assert!(handler.get_user_from_ssrc(100).is_none());
        assert!(handler.get_user_from_ssrc(200).is_none());
    }

    #[tokio::test]
    async fn test_mute_toggle() {
        let handler = VoiceHandler::new();
        assert!(!handler.is_muted().await);
        handler.set_muted(true).await;
        assert!(handler.is_muted().await);
        handler.set_muted(false).await;
        assert!(!handler.is_muted().await);
    }

    #[tokio::test]
    async fn test_add_audio_raw_updates_buffer() {
        let handler = VoiceHandler::new();
        handler.set_ssrc_user(100, 42);
        let audio = vec![1000i16; 1600]; // 0.1s at 16kHz
        handler.add_audio_raw(&audio, 100).await;

        let buffer = handler.audio_buffer.lock().await;
        assert_eq!(buffer.len(), 1600);
    }

    #[tokio::test]
    async fn test_add_audio_raw_per_user_buffer() {
        let handler = VoiceHandler::new();
        handler.set_ssrc_user(100, 42);
        handler.set_ssrc_user(200, 99);

        handler.add_audio_raw(&vec![1000i16; 100], 100).await;
        handler.add_audio_raw(&vec![2000i16; 200], 200).await;

        let buf_100 = handler.user_buffers.get(&100).unwrap();
        assert_eq!(buf_100.len(), 100);
        let buf_200 = handler.user_buffers.get(&200).unwrap();
        assert_eq!(buf_200.len(), 200);
    }

    #[tokio::test]
    async fn test_transcription_channel() {
        let handler = VoiceHandler::new();
        let (tx, mut rx) = mpsc::channel::<(String, Vec<i16>)>(10);
        handler.set_transcription_channel(tx).await;
        handler.set_ssrc_user(100, 42);

        let audio = vec![1000i16; 16000]; // 1s at 16kHz
        handler.add_audio_raw(&audio, 100).await;

        {
            let mut last = handler.last_speech_time.lock().await;
            *last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        }

        handler.process_audio(100).await;

        let msg = rx.recv().await;
        assert!(msg.is_some());
        let (user_id, _) = msg.unwrap();
        assert_eq!(user_id, "42");
    }

    #[tokio::test]
    async fn test_process_audio_no_ssrc_mapping() {
        let handler = VoiceHandler::new();
        let (tx, _rx) = mpsc::channel::<(String, Vec<i16>)>(10);
        handler.set_transcription_channel(tx).await;

        // Add audio without SSRC mapping
        let audio = vec![1000i16; 16000];
        handler.add_audio_raw(&audio, 999).await;

        // Reset speech time
        {
            let mut last = handler.last_speech_time.lock().await;
            *last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        }

        // Should not panic, just log warning
        handler.process_audio(999).await;
    }

    #[tokio::test]
    async fn test_process_audio_too_short() {
        let handler = VoiceHandler::new();
        handler.set_ssrc_user(100, 42);
        let (tx, mut rx) = mpsc::channel::<(String, Vec<i16>)>(10);
        handler.set_transcription_channel(tx).await;

        // Add very short audio (below minimum)
        let audio = vec![1000i16; 100]; // ~2ms
        handler.add_audio_raw(&audio, 100).await;

        {
            let mut last = handler.last_speech_time.lock().await;
            *last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        }

        handler.process_audio(100).await;

        // Should not have sent anything
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_songbird_integration_types() {
        // Verify VoiceReceiver is Clone
        let handler = Arc::new(VoiceHandler::new());
        let receiver = songbird_integration::VoiceReceiver::new(handler.clone());
        let _clone = receiver.clone();
    }
}
