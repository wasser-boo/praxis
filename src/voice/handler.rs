use crate::voice::{STTEngineType, TTSEngineType};

pub struct VoiceHandler {
    stt_engine: Option<STTEngineType>,
    tts_engine: Option<TTSEngineType>,
    muted: bool,
    deafened: bool,
}

impl VoiceHandler {
    pub fn new() -> Self {
        Self {
            stt_engine: None,
            tts_engine: None,
            muted: false,
            deafened: true,
        }
    }

    pub fn with_stt(mut self, engine: STTEngineType) -> Self {
        self.stt_engine = Some(engine);
        self
    }

    pub fn with_tts(mut self, engine: TTSEngineType) -> Self {
        self.tts_engine = Some(engine);
        self
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn set_deafened(&mut self, deafened: bool) {
        self.deafened = deafened;
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn is_deafened(&self) -> bool {
        self.deafened
    }

    pub fn stt_engine(&self) -> Option<&STTEngineType> {
        self.stt_engine.as_ref()
    }

    pub fn tts_engine(&self) -> Option<&TTSEngineType> {
        self.tts_engine.as_ref()
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    #[test]
    fn test_voice_handler_creation() {
        let handler = VoiceHandler::new();
        assert!(!handler.is_muted());
        assert!(handler.is_deafened());
        assert!(handler.stt_engine().is_none());
        assert!(handler.tts_engine().is_none());
    }

    #[test]
    fn test_voice_handler_builder() {
        let handler = VoiceHandler::new()
            .with_stt(STTEngineType::Vosk)
            .with_tts(TTSEngineType::ElevenLabs);
        assert!(handler.stt_engine().is_some());
        assert!(handler.tts_engine().is_some());
    }

    #[test]
    fn test_voice_handler_mute() {
        let mut handler = VoiceHandler::new();
        handler.set_muted(true);
        assert!(handler.is_muted());
        handler.set_muted(false);
        assert!(!handler.is_muted());
    }

    #[test]
    fn test_voice_handler_deafen() {
        let mut handler = VoiceHandler::new();
        handler.set_deafened(false);
        assert!(!handler.is_deafened());
        handler.set_deafened(true);
        assert!(handler.is_deafened());
    }
}
