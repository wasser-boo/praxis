use praxis::voice::tts;
use praxis::voice;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("tts");

    match mode {
        "tts" => test_tts().await?,
        "stt" => test_stt().await?,
        "both" => {
            test_tts().await?;
            println!("\n---\n");
            test_stt().await?;
        }
        _ => {
            println!("Usage: cargo run --example test_tts -- [tts|stt|both]");
            println!("  tts  - Test text-to-speech engines");
            println!("  stt  - Test speech-to-text engines");
            println!("  both - Test both");
        }
    }

    Ok(())
}

async fn test_tts() -> anyhow::Result<()> {
    println!("=== TTS Test ===\n");

    let text = "Hello, this is a test of the text to speech system.";

    // Test Windows SAPI (only works on Windows)
    #[cfg(target_os = "windows")]
    {
        println!("Testing Windows SAPI...");
        let sapi = tts::windows_sapi::WindowsSAPI::new();
        match sapi.speak_to_bytes(text) {
            Ok(bytes) => {
                println!("  Windows SAPI: {} bytes", bytes.len());
                let path = tts::save_audio_file(&bytes, ".", "test_sapi")?;
                println!("  Saved to: {:?}", path);
            }
            Err(e) => println!("  Windows SAPI failed: {}", e),
        }
    }

    // Test ElevenLabs
    if let (Ok(api_key), Ok(voice_id)) = (
        std::env::var("ELEVENLABS_API_KEY"),
        std::env::var("ELEVENLABS_VOICE_ID"),
    ) {
        println!("Testing ElevenLabs TTS...");
        let el = tts::elevenlabs::ElevenLabsTTS::new(api_key, voice_id);
        match el.speak(text).await {
            Ok(bytes) => {
                println!("  ElevenLabs: {} bytes", bytes.len());
                let path = tts::save_audio_file(&bytes, ".", "test_elevenlabs")?;
                println!("  Saved to: {:?}", path);

                let pcm = tts::audio_bytes_to_pcm(&bytes)?;
                println!("  PCM samples: {}", pcm.len());
            }
            Err(e) => println!("  ElevenLabs failed: {}", e),
        }
    } else {
        println!("Skipping ElevenLabs: set ELEVENLABS_API_KEY + ELEVENLABS_VOICE_ID");
    }

    // Test MiniMax
    if let (Ok(api_key), Ok(voice_id)) = (
        std::env::var("MINIMAX_API_KEY"),
        std::env::var("MINIMAX_VOICE_ID"),
    ) {
        println!("Testing MiniMax TTS...");
        let model = std::env::var("MINIMAX_MODEL").unwrap_or_else(|_| "speech-02-hd".to_string());
        let mm = tts::minimax_tts::MiniMaxTTS::new(api_key, voice_id, model);
        match mm.speak(text).await {
            Ok(bytes) => {
                println!("  MiniMax: {} bytes", bytes.len());
                let _ = tts::save_audio_file(&bytes, ".", "test_minimax");
            }
            Err(e) => println!("  MiniMax failed: {}", e),
        }
    } else {
        println!("Skipping MiniMax: set MINIMAX_API_KEY + MINIMAX_VOICE_ID");
    }

    // Test MiMo
    if let Ok(api_key) = std::env::var("MIMO_API_KEY") {
        println!("Testing MiMo TTS...");
        let mimo = tts::mimo_tts::MiMoTTS::new(
            api_key,
            "mimo-v2.5-tts".to_string(),
            std::env::var("MIMO_TTS_API_BASE").ok(),
        );
        let voice = std::env::var("MIMO_VOICE").unwrap_or_else(|_| "mimo_default".to_string());
        match mimo.speak_builtin(text, &voice, None).await {
            Ok(bytes) => {
                println!("  MiMo: {} bytes", bytes.len());
                let _ = tts::save_audio_file(&bytes, ".", "test_mimo");
            }
            Err(e) => println!("  MiMo failed: {}", e),
        }
    } else {
        println!("Skipping MiMo: set MIMO_API_KEY");
    }

    // Test Qwen TTS
    if let Ok(server) = std::env::var("QWEN_TTS_SERVER") {
        println!("Testing Qwen TTS...");
        let language = std::env::var("QWEN_TTS_LANGUAGE").unwrap_or_else(|_| "English".to_string());
        let qwen = tts::qwen_tts::QwenTTSClient::new(server, language, std::env::var("QWEN_TTS_SPEAKER").ok());
        match qwen.speak(text).await {
            Ok(bytes) => {
                println!("  Qwen: {} bytes", bytes.len());
                let _ = tts::save_audio_file(&bytes, ".", "test_qwen");
            }
            Err(e) => println!("  Qwen failed: {}", e),
        }
    } else {
        println!("Skipping Qwen: set QWEN_TTS_SERVER");
    }

    println!("\nTTS test complete. Check *.wav files in current directory.");
    Ok(())
}

async fn test_stt() -> anyhow::Result<()> {
    println!("=== STT Test ===\n");

    // Generate a test WAV file with silence + tone
    let sample_rate = 16000u32;
    let duration_secs = 2.0f32;
    let samples: Vec<i16> = (0..(sample_rate as f32 * duration_secs) as i32)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            // 440Hz sine wave, quiet
            (t * 440.0 * 2.0 * std::f32::consts::PI).sin() as i16 / 4
        })
        .collect();

    let wav_data = voice::pcm_to_wav(&samples, sample_rate, 1);
    println!("Generated test WAV: {} bytes ({} samples, {}Hz, {:.1}s)",
        wav_data.len(), samples.len(), sample_rate, duration_secs);

    // Test ElevenLabs STT
    if let Ok(api_key) = std::env::var("ELEVENLABS_API_KEY") {
        println!("\nTesting ElevenLabs STT...");
        let el_stt = voice::ElevenLabsSTT::new(api_key);
        match el_stt.transcribe(&wav_data).await {
            Ok(text) => println!("  ElevenLabs STT result: '{}'", text),
            Err(e) => println!("  ElevenLabs STT failed: {}", e),
        }
    } else {
        println!("Skipping ElevenLabs STT: set ELEVENLABS_API_KEY");
    }

    // Test Vosk STT (requires feature flag)
    #[cfg(feature = "voice_vosk")]
    {
        if let Ok(model_path) = std::env::var("VOSK_MODEL_PATH") {
            println!("\nTesting Vosk STT...");
            let vosk = voice::VoskSTT::new(Some(model_path));
            match vosk.load_model().await {
                Ok(()) => {
                    println!("  Vosk model loaded");
                    match vosk.transcribe(&samples).await {
                        Ok(text) => println!("  Vosk STT result: '{}'", text),
                        Err(e) => println!("  Vosk STT failed: {}", e),
                    }
                }
                Err(e) => println!("  Vosk model load failed: {}", e),
            }
        } else {
            println!("Skipping Vosk: set VOSK_MODEL_PATH");
        }
    }
    #[cfg(not(feature = "voice_vosk"))]
    {
        println!("\nSkipping Vosk: build with --features voice_vosk");
    }

    // Test Whisper STT (requires feature flag)
    #[cfg(feature = "voice_whisper")]
    {
        if let Ok(model_path) = std::env::var("WHISPER_MODEL_PATH") {
            println!("\nTesting Whisper STT...");
            let whisper = voice::WhisperSTT::new(Some(model_path));
            match whisper.load_model().await {
                Ok(()) => println!("  Whisper model loaded"),
                Err(e) => println!("  Whisper model load failed: {}", e),
            }
        } else {
            println!("Skipping Whisper: set WHISPER_MODEL_PATH");
        }
    }
    #[cfg(not(feature = "voice_whisper"))]
    {
        println!("\nSkipping Whisper: build with --features voice_whisper");
    }

    // Test the unified transcribe_audio function
    println!("\nTesting unified transcribe_audio...");
    match voice::transcribe_audio(&wav_data, "elevenlabs", std::env::var("ELEVENLABS_API_KEY").ok().as_deref(), None).await {
        Ok(text) => println!("  transcribe_audio result: '{}'", text),
        Err(e) => println!("  transcribe_audio failed: {}", e),
    }

    // Test audio conversion utilities
    println!("\n=== Audio Utilities ===");
    println!("pcm_to_wav: {} samples -> {} bytes WAV", samples.len(), wav_data.len());
    println!("get_wav_sample_rate: {:?}", voice::get_wav_sample_rate(&wav_data));

    let downsampled = voice::downsample_48k_to_16k(&samples);
    println!("downsample_48k_to_16k: {} -> {} samples", samples.len(), downsampled.len());

    let gated = voice::apply_noise_gate(&samples, 100);
    let non_zero_original = samples.iter().filter(|&&s| s != 0).count();
    let non_zero_gated = gated.iter().filter(|&&s| s != 0).count();
    println!("apply_noise_gate: {}/{} -> {}/{} non-zero samples",
        non_zero_original, samples.len(), non_zero_gated, gated.len());

    println!("\nSTT test complete.");
    Ok(())
}
