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

**Browser limitation:** the dashboard STT route can submit PCM WAV without an
ElevenLabs key, but the current browser MediaRecorder usually produces WebM/Opus
or Ogg/Opus. Those blobs must be converted to PCM WAV before calling remote Vosk.
This change does not replace browser recording/audio handling or silently send
encoded audio as PCM. No local ffmpeg or codec executable is added as a dependency.
Discord's existing WAV input needs no extra conversion.

## Tests and live gate

```sh
cargo test --locked --lib vosk_remote
cargo check --locked --features songbird
python3 scripts/check_context_docs.py
```

Local WebSocket mocks check the standard protocol, transcription without a local
model/key, sample rate/downmix, partial/final results, malformed/error responses,
disconnects, timeout/cancellation and configuration persistence. Run the tests
without `voice_vosk` to establish independence from local Vosk.

A real remote Vosk endpoint has not been provided/verified here. Deploy a compatible
server with the desired language model, verify private reachability, then test
Discord transcription and language accuracy end-to-end before relying on it.
