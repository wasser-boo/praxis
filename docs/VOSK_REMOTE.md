# Remote Vosk STT

Praxis can use a **standard vosk-server WebSocket endpoint** instead of installing
Vosk locally. This is a separate STT server, not a ComfyUI node or an HTTP `/stt`
endpoint. No Vosk service/model is installed or started by Praxis.

```text
/context set settings.voice_stt_type=vosk
/context set settings.voice_vosk_url=ws://100.80.1.2:2700
```

Alternatively set `VOSK_SERVER_URL=ws://REMOTE_IP:2700` in the Praxis process
environment. Nonempty per-context `settings.voice_vosk_url` takes precedence;
null/empty inherits the environment. If neither is set, **existing local Vosk
behavior** is retained. Remote errors never silently fall back to a local model.
To select local Vosk again, clear both the context URL and environment default.

- Remote mode works in a normal build **without** `--features voice_vosk`, libvosk,
  Python Vosk packages, or a local model directory. The remote server owns those.
- Keep existing Discord `voice_enabled`, `use_stt`, `/join`, pairing and microphone
  permissions. For example `/context set settings.voice_enabled=true settings.use_stt=true`.
  Discord supplies 16 kHz mono PCM WAV through its existing audio pipeline.
- `wss://host/path` supports TLS with normal certificate validation. `ws://IP:port`
  supports private remote IPs (including NetBird); do not expose an unauthenticated
  Vosk server publicly. Apply a peer/port-specific NetBird ACL, e.g. TCP 2700 only
  from Praxis. No firewall/listener/deployment changes are made automatically.
- URL credentials, query and fragment are rejected. The client does not reconnect,
  redirect, retry, choose another provider, or control other clients' recognition.
- `VOSK_TIMEOUT_SECONDS` is an integer 1–600, default 120, bounding the entire
  connection/transcription. Connection establishment is additionally limited to
  10 seconds. Cancellation/future drop releases only this request's connection.

## Audio and protocol

The provider accepts **16-bit integer PCM WAV, mono or stereo, 8–48 kHz**, at most
16 MiB / 5 minutes. It reuses Praxis's existing WAV decode/downmix helper, sends
`{"config":{"sample_rate":ACTUAL_RATE}}`, then binary little-endian mono PCM in
4000-sample chunks, reading each server response, then `{"eof":1}` and the final
result. Finalized segments are joined; partial hypotheses are not duplicated.
Empty finalized text is a valid silence result. JSON/frame and transcript sizes
are bounded. Vosk word confidence is not treated as ElevenLabs language confidence.

**Dashboard microphone:** when this chat selects `vosk`, the browser decodes its
MediaRecorder container (for example WebM/Opus), downmixes and resamples it to
**mono 16 kHz PCM16 WAV** before upload. Vosk browser recordings stop after **60
seconds**, keeping WAV plus multipart overhead below the existing 2 MiB dashboard
request limit. Other STT providers keep their original encoded upload format.
No local ffmpeg or codec executable is required. Discord already supplies WAV.

Recording and transcription stay associated with the chat in which recording
started; a late result is never inserted into a different chat. The recognized
text appears in the input box for review: **press Send to get an answer**. Vosk
itself produces no audible reply; configure TTS separately (for example
`comfyui_qwen3`), enable reply audio, and allow browser playback. Reload an open
dashboard after updating its JavaScript.

The `praxis-gpu` companion serves native WebSockets at `/`, `/ws`, `/de`, `/fr`,
`/ja` and `/ws/LANGUAGE`, while preserving legacy HTTP WAV `/transcribe`.
Choose a model per context, for example:

```text
/context set settings.voice_stt_type=vosk settings.voice_vosk_url=ws://100.80.1.2:2700/fr
```

Use `/de` for German and `/ja` for Japanese. These are companion-server routes,
not requirements imposed on every third-party vosk-server. Do not use query
strings with the native Praxis client. Vosk uses a **fixed language model per
connection**, not reliable automatic DE/FR or DE/JA code-switch detection.
A successful HTTP `/health` check alone does not establish WebSocket support.

## Tests and live gate

```sh
cargo test --locked --lib vosk_remote
cargo check --locked --features songbird
python3 scripts/check_context_docs.py
node tests/test_browser_wav.js
node tests/test_browser_mic.js
# With Playwright/Chromium installed:
node scripts/test_browser_mic.js
node scripts/test_chat_audio.js
```

Local WebSocket mocks check the standard protocol, transcription without a local
model/key, sample rate/downmix, partial/final results, malformed/error responses,
disconnects, timeout/cancellation and configuration persistence. Run the tests
without `voice_vosk` to establish independence from local Vosk.

The browser codec tests use synthetic MediaRecorder audio and stereo WAV, not a
microphone or paid service. The GPU companion's DE/FR/JA WebSocket handshake,
per-chunk acknowledgements and EOF behavior, legacy HTTP, and native dashboard
DE/FR recognition have also been exercised over a private VPN. This does **not**
verify a user's microphone permissions, Discord capture, or audible playback.
For Discord, check pairing, `voice_enabled`, `use_stt`, `voice_deafened=false`,
and `/join`; server-side recognition success is not proof that the bot is in the
voice channel. Preserve wake-word preferences rather than enabling all listening
implicitly.
