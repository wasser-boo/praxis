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
    pub transcription_tx: Arc<Mutex<Option<mpsc::Sender<(u64, Vec<i16>)>>>>,
    pub user_buffers: Arc<DashMap<u32, Vec<i16>>>,
    pub user_last_speech: Arc<DashMap<u32, std::time::Instant>>,
}

impl VoiceHandler {
    pub fn new() -> Self {
        Self {
            states: Arc::new(DashMap::new()),
            muted: Arc::new(Mutex::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            last_speech_time: Arc::new(Mutex::new(std::time::Instant::now())),
            last_active_ssrc: Arc::new(Mutex::new(None)),
            sample_rate: 48000,
            ssrc_to_user: Arc::new(DashMap::new()),
            transcription_tx: Arc::new(Mutex::new(None)),
            user_buffers: Arc::new(DashMap::new()),
            user_last_speech: Arc::new(DashMap::new()),
        }
    }

    pub async fn set_transcription_channel(&self, tx: mpsc::Sender<(u64, Vec<i16>)>) {
        let mut lock = self.transcription_tx.lock().await;
        *lock = Some(tx);
    }

    pub fn set_ssrc_user(&self, ssrc: u32, user_id: u64) {
        self.ssrc_to_user.insert(ssrc, user_id);
        tracing::debug!("SSRC {} -> user {}", ssrc, user_id);
    }

    pub fn get_user_from_ssrc(&self, ssrc: u32) -> Option<u64> {
        self.ssrc_to_user.get(&ssrc).map(|r| *r.value())
    }

    pub fn remove_user(&self, user_id: u64) {
        self.ssrc_to_user.retain(|_, &mut v| v != user_id);
        tracing::debug!("Removed user {} from SSRC mapping", user_id);
    }

    pub async fn add_audio_raw(&self, audio: &[i16], ssrc: u32) {
        self.user_buffers.entry(ssrc).or_insert_with(Vec::new).extend_from_slice(audio);
        self.user_last_speech.insert(ssrc, std::time::Instant::now());

        let mut buffer = self.audio_buffer.lock().await;
        buffer.extend_from_slice(audio);

        let has_content = audio.iter().any(|&s| s.abs() > 100);
        if has_content {
            let mut last_time = self.last_speech_time.lock().await;
            *last_time = std::time::Instant::now();
            drop(last_time);
            let mut last_ssrc = self.last_active_ssrc.lock().await;
            *last_ssrc = Some(ssrc);
            tracing::debug!("VOICE: Speech detected (ssrc={}, buffer={} samples)", ssrc,
                self.user_buffers.get(&ssrc).map(|b| b.len()).unwrap_or(0));
        }
    }

    pub async fn process_audio(&self, current_ssrc: u32) {
        let time_since_speech = self.time_since_last_speech().await;
        let pause_threshold = std::time::Duration::from_secs_f32(0.8);

        if time_since_speech < pause_threshold {
            tracing::trace!("VOICE_HANDLER: Still speaking ({}ms since last audio), waiting for pause...",
                time_since_speech.as_millis());
            return;
        }

        if self.get_user_from_ssrc(current_ssrc).is_none() {
            tracing::trace!("VOICE_HANDLER: SSRC {} not mapped yet, keeping buffer", current_ssrc);
            return;
        }

        let (buffer_len, audio_data) = if let Some(mut entry) = self.user_buffers.get_mut(&current_ssrc) {
            let len = entry.len();
            let data = entry.clone();
            entry.clear();
            (len, data)
        } else {
            let mut buffer = self.audio_buffer.lock().await;
            let len = buffer.len();
            let data = buffer.clone();
            buffer.clear();
            (len, data)
        };

        let min_duration_secs = 0.3;
        let min_samples = (self.sample_rate as f32 * min_duration_secs) as usize;

        if buffer_len < min_samples {
            tracing::trace!("VOICE_HANDLER: Buffer only {} samples (min {}), ignoring", buffer_len, min_samples);
            return;
        }

        tracing::info!("VOICE_HANDLER: Pause detected ({}ms), sending {} samples for transcription (ssrc={})",
            time_since_speech.as_millis(), buffer_len, current_ssrc);
        if let Some(user_id) = self.get_user_from_ssrc(current_ssrc) {
            let tx_guard = self.transcription_tx.lock().await;
            if let Some(tx) = tx_guard.as_ref() {
                tracing::info!("VOICE_HANDLER: SSRC {} -> user {}, sending to transcription channel", current_ssrc, user_id);
                let _ = tx.send((user_id, audio_data)).await;
            } else {
                tracing::warn!("VOICE_HANDLER: transcription_tx is None!");
            }
        } else {
            tracing::warn!("VOICE_HANDLER: No user_id for SSRC {} - audio discarded ({} samples). Waiting for SpeakingStateUpdate.", current_ssrc, buffer_len);
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
    use songbird::{Config, driver::DecodeMode};
    use async_trait::async_trait;

    pub fn create_songbird_config() -> Config {
        Config::default().decode_mode(DecodeMode::Decode(Default::default()))
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
                    tracing::trace!("VOICETICK: SpeakingStateUpdate: ssrc={}, user_id={:?}, speaking={:?}", speaking.ssrc, speaking.user_id, speaking.speaking);
                    if let Some(user_id) = speaking.user_id {
                        self.handler.set_ssrc_user(speaking.ssrc, user_id.0);
                    }
                },
                EventContext::VoiceTick(tick) => {
                    let speaking_count = tick.speaking.len();
                    let silent_count = tick.silent.len();
                    if speaking_count > 0 {
                        tracing::debug!("VOICETICK: {} user(s) speaking", speaking_count);
                    }
                    for (ssrc, data) in &tick.speaking {
                        let user_id_str = self.handler.get_user_from_ssrc(*ssrc).map(|u| u.to_string()).unwrap_or_else(|| "?".into());
                        tracing::trace!("VOICETICK: Processing SSRC {}/{}", ssrc, user_id_str);
                        if self.handler.get_user_from_ssrc(*ssrc).is_none() {
                            tracing::warn!("VOICETICK: Unknown SSRC {} - no user mapping yet, buffering audio anyway", ssrc);
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
                    let ssrc_to_use = tick.speaking.iter().next().map(|(s, _)| *s)
                        .or_else(|| {
                            self.handler.last_active_ssrc.try_lock().ok().and_then(|g| g.as_ref().copied())
                        });
                    if let Some(ssrc) = ssrc_to_use {
                        let _ = self.handler.process_audio(ssrc).await;
                    }
                },
                EventContext::ClientDisconnect(disc) => {
                    tracing::debug!("Client disconnect: user_id={}", disc.user_id);
                    if disc.user_id.0 != 0 {
                        self.handler.remove_user(disc.user_id.0);
                    }
                },
                _ => {}
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
        assert_eq!(handler.sample_rate, 48000);
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
        let audio = vec![1000i16; 4800]; // 0.1s at 48kHz
        handler.add_audio_raw(&audio, 100).await;

        let buffer = handler.audio_buffer.lock().await;
        assert_eq!(buffer.len(), 4800);
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
        let (tx, mut rx) = mpsc::channel::<(u64, Vec<i16>)>(10);
        handler.set_transcription_channel(tx).await;
        handler.set_ssrc_user(100, 42);

        // Add enough audio to exceed minimum
        let audio = vec![1000i16; 48000]; // 1s at 48kHz
        handler.add_audio_raw(&audio, 100).await;

        // Reset last_speech_time to simulate pause
        {
            let mut last = handler.last_speech_time.lock().await;
            *last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        }

        handler.process_audio(100).await;

        let msg = rx.recv().await;
        assert!(msg.is_some());
        let (user_id, _) = msg.unwrap();
        assert_eq!(user_id, 42);
    }

    #[tokio::test]
    async fn test_process_audio_no_ssrc_mapping() {
        let handler = VoiceHandler::new();
        let (tx, _rx) = mpsc::channel::<(u64, Vec<i16>)>(10);
        handler.set_transcription_channel(tx).await;

        // Add audio without SSRC mapping
        let audio = vec![1000i16; 48000];
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
        let (tx, mut rx) = mpsc::channel::<(u64, Vec<i16>)>(10);
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
