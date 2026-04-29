pub mod handler;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum STTEngineType {
    Vosk,
    Whisper,
    ElevenLabs,
}

impl STTEngineType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "vosk" => Some(STTEngineType::Vosk),
            "whisper" => Some(STTEngineType::Whisper),
            "elevenlabs" => Some(STTEngineType::ElevenLabs),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TTSEngineType {
    WindowsSapi,
    ElevenLabs,
    QwenTts,
    MiniMax,
    MiMo,
}

impl TTSEngineType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "windows_sapi" => Some(TTSEngineType::WindowsSapi),
            "elevenlabs" => Some(TTSEngineType::ElevenLabs),
            "qwen_tts" => Some(TTSEngineType::QwenTts),
            "minimax" => Some(TTSEngineType::MiniMax),
            "mimo" => Some(TTSEngineType::MiMo),
            _ => None,
        }
    }
}

pub fn pcm_to_wav(samples: &[i16], sample_rate: u32, channels: u16) -> Vec<u8> {
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_size = samples.len() as u32 * 2;
    let file_size = 36 + data_size;

    let mut wav = Vec::with_capacity(44 + data_size as usize);

    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());

    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }

    wav
}

pub fn get_wav_sample_rate(data: &[u8]) -> Option<u32> {
    if data.len() < 28 {
        return None;
    }
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    Some(u32::from_le_bytes([data[24], data[25], data[26], data[27]]))
}

pub fn downsample_48k_to_16k(samples: &[i16]) -> Vec<i16> {
    let ratio = 3;
    samples.iter().step_by(ratio).copied().collect()
}

pub fn apply_noise_gate(samples: &[i16], threshold: i16) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| if s.abs() < threshold { 0 } else { s })
        .collect()
}

pub async fn transcribe_audio(
    _audio_data: &[u8],
    stt_type: &str,
    _api_key: Option<&str>,
    _model_path: Option<&str>,
) -> Result<String, String> {
    match stt_type {
        "vosk" => Err("Vosk support not compiled. Build with --features voice_vosk".to_string()),
        "whisper" => Err("Whisper support not compiled. Build with --features voice_whisper".to_string()),
        "elevenlabs" => Err("ElevenLabs STT not yet implemented".to_string()),
        _ => Err(format!("Unknown STT type: {}", stt_type)),
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    #[test]
    fn test_pcm_to_wav() {
        let samples = vec![100, -100, 200, -200];
        let wav = pcm_to_wav(&samples, 44100, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
    }

    #[test]
    fn test_get_wav_sample_rate() {
        let samples = vec![0i16; 100];
        let wav = pcm_to_wav(&samples, 48000, 1);
        assert_eq!(get_wav_sample_rate(&wav), Some(48000));
    }

    #[test]
    fn test_downsample() {
        let samples: Vec<i16> = (0..48000).map(|i| (i % 32000) as i16).collect();
        let downsampled = downsample_48k_to_16k(&samples);
        assert_eq!(downsampled.len(), 16000);
    }

    #[test]
    fn test_noise_gate() {
        let samples = vec![50, -50, 500, -500, 10, -10];
        let cleaned = apply_noise_gate(&samples, 100);
        assert_eq!(cleaned, vec![0, 0, 500, -500, 0, 0]);
    }

    #[test]
    fn test_stt_engine_type_from_str() {
        assert!(STTEngineType::from_str("vosk").is_some());
        assert!(STTEngineType::from_str("whisper").is_some());
        assert!(STTEngineType::from_str("elevenlabs").is_some());
        assert!(STTEngineType::from_str("unknown").is_none());
    }

    #[test]
    fn test_tts_engine_type_from_str() {
        assert!(TTSEngineType::from_str("windows_sapi").is_some());
        assert!(TTSEngineType::from_str("elevenlabs").is_some());
        assert!(TTSEngineType::from_str("unknown").is_none());
    }
}
