# Discord voice + French/Japanese learning

## Required result

The acceptance test is **Discord microphone → ElevenLabs STT → configured LLM → ElevenLabs TTS → audible Discord voice playback**. A successful HTTP-only speech test or POML render is not a substitute for this test.

Runtime directory: **`/workspace/release`**. Source/build checkout: `/workspace/praxis`.

The existing paired context is stored in `/workspace/release/data/praxis.db`; workflow/preset files in `contexts/` are not automatically loaded as database contexts. The existing `.env`, encrypted `secrets.enc2`, and `.secrets_salt` must remain together. Do **not** rerun onboarding to apply this fix: it can replace configuration/templates.

## Start the deployed build

Stop the old Praxis process normally before starting another copy; do not interrupt Ollama or the VM independently. Then:

```bash
cd /workspace/release
./praxis run
```

Enter the existing master password directly at the terminal prompt, never as a command-line argument or in chat. Do not add `--no-discord`. The binary must be built with:

```bash
cargo build --release --locked --features songbird --jobs 1
```

The startup code explicitly selects the AWS-LC Rustls provider before creating TLS clients. Both `ring` and `aws-lc-rs` are pulled in by dependencies, so leaving selection to automatic feature detection caused the voice-worker panic. An already selected provider is also accepted by the optional HTTPS dashboard.

## Existing ElevenLabs configuration

The voice switches, ElevenLabs engines, voice ID, and multilingual models were already set in the active paired context when inspected. Preserve the selected voice and wake word. All fields, allowed values, defaults, and implementation caveats are documented in [CONTEXT_VARIABLES.md](CONTEXT_VARIABLES.md).

The necessary settings are:

```json
{
  "settings.voice_enabled": true,
  "settings.use_stt": true,
  "settings.voice_stt_type": "elevenlabs",
  "settings.voice_deafened": false,
  "settings.use_tts": true,
  "settings.voice_tts_type": "elevenlabs",
  "settings.elevenlabs_stt_model": "scribe_v2",
  "settings.elevenlabs_tts_model": "eleven_multilingual_v2",
  "settings.elevenlabs_stt_language": null,
  "settings.elevenlabs_tts_language": null,
  "settings.system_template": "language_learning"
}
```

This is a **dotted context patch**, not an `.env` file. Keep your existing non-empty `settings.voice_elevenlabs_voice_id`. Both STT and TTS use Secrets → **`elevenlabs_api_key`**. The old secret aliases `voice_elevenlabs_api_key` and `voice_elevenlabs_stt_api_key` are not used by this path. Save Secrets with the master password to persist them, then restart because components keep startup copies.

Important details:

- `voice_tts_enabled` alone does not enable replies; the working switch is `use_tts`.
- `voice_deafened=false` is applied during `/join`; rejoin after changing it. Discord server/self mute/deafen controls also matter.
- Your configured wake word must be spoken. `/context` → `get settings.voice_wake_words` shows it. `["*"]` forwards every transcript, while `[]` forwards none. Do not replace a chosen trigger with `*` unless that is what you want.
- Wake-word matching happens **after** STT, so even ignored speech from an enabled paired speaker can consume transcription credits.
- Leave both language overrides null for French/Japanese/German practice. The TTS language setting controls pronunciation; it does not translate text.
- `voice_auto_pause_enabled=false` uses speech-ending events. True adds 1.5-second silence/3-second buffer flushing and can split longer sentences. This choice is captured on join.
- RVC is not necessary; keep `rvc_on=false` unless a separate working RVC service is intended.
- Vosk is not available in a Songbird-only build, Windows SAPI does not work on Linux, and the current Whisper implementation is a stub. Selecting those is not an alternative working configuration here.

## Language-learning template

File: `/workspace/release/templates/language_learning.poml`.

It teaches **French and Japanese**, uses **German explanations/translations when help is needed**, gives concise corrections and practice questions, and asks for clarification when the language/transcript is unclear. Responses are kept speech-friendly. It does not claim to assess pronunciation from a text transcript alone.

Select it in the intended context:

```text
/context set settings.system_template=language_learning
```

Use the name without `.poml`. Your customized `templates/system.poml` is preserved. To switch back, set `settings.system_template=system` (or null for the default). The new template safely handles an absent optional `user_prompt`; the message itself remains in conversation history.

## Discord live acceptance test

This test uses the configured model and ElevenLabs services and may incur their normal charges. Start it when you are ready, not as an unattended background test.

1. Confirm startup reports **Discord bot connected** and **Songbird manager stored** without a Rustls panic.
2. Use the Discord account already paired to the intended Praxis context. Ensure the bot has **View Channel**, **Connect**, and **Speak** permissions. The current bot also requests the Message Content and Server Members intents; enable required privileged intents in the developer portal.
3. Enter your voice channel and run `/join`. Ensure the bot is not server-muted or deafened.
4. Say your configured wake word followed by a short French request, for example “Bonjour, aide-moi à pratiquer le français.” Pause at the end.
5. Confirm the transcript is recognized, a reply is generated, and you **hear the reply in that same voice channel**.
6. Say a second, different request. Confirm the bot answers the new request, not the previous one. The voice handler now consumes progress/“Thinking...” frames until the final gateway response, avoiding the old one-turn lag.
7. Try a short Japanese request and ask for a German explanation. Confirm text and audio are not corrupted. Ollama byte buffering must preserve multibyte UTF-8 and must not erase accumulated text on an empty final chunk.
8. `/disconnect` should remove the bot from voice when finished.

Relevant log sequence (wording may vary slightly):

```text
Joined voice channel ... with STT pipeline
VOICE_PIPELINE: Received ... samples
STT: Starting transcription with engine 'elevenlabs'
VOICE_PIPELINE: Transcribed ...
VOICE_PIPELINE: Got response ...
Voice TTS for user ...
TTS decoded: ... samples at ... Hz
TTS audio playing in guild ...
TTS audio finished in guild ...
```

Logs can contain transcripts; do not paste secrets or private conversations into shared diagnostics. Seeing a playback log does not alone prove you heard sound; confirm the audible result in Discord.

### Troubleshooting by stage

| Symptom | Check |
|---|---|
| Rustls `CryptoProvider` panic | Old executable still running; restart the newly deployed Songbird build. |
| Bot never joins | Pairing, voice-channel membership/permissions, Discord credentials and connection; run `/join` again after restart. |
| Joined but no sample/transcription logs | Actual Discord deafening, `voice_enabled`, `use_stt`, runtime mute, and SSRC-to-paired-user mapping. |
| STT 401/403 or quota error | Correct shared ElevenLabs secret, permissions/credits, persisted save and restart. A key's presence does not prove access. |
| Transcript exists but no LLM request | Wake-word mismatch/empty wake-word list; use the chosen trigger, not an assumed bot display name. |
| No final reply or wrong-turn text | Gateway/model errors and progress handling; inspect the updated voice gateway path. |
| Reply text exists but no speech | `use_tts`, `voice_tts_type`, voice ID, ElevenLabs access/model support, and active voice connection. |
| Empty/invalid speech response | Provider error/empty audio; the client now reports it instead of accepting empty output. |
| Garbled French/Japanese with Ollama | Confirm the newly built executable includes byte-safe NDJSON handling. |
| Audio generated but inaudible | Bot/Discord output mute, Speak permission, client output volume, correct active guild/channel. |

**Current architectural limitation:** playback uses one process-wide active voice-guild state (the most recent join), not a per-context guild selector. Test/use one active guild at a time. `settings.voice_discord_guild_id` does not override that routing.

## Local regression checks — no real provider requests

From `/workspace/praxis`:

```bash
source ~/.cargo/env
cargo test --release --locked --offline --features songbird --bin praxis --jobs 1
cargo test --release --locked --offline --features songbird --lib --jobs 1 voice -- --test-threads=1
python3 scripts/check_context_docs.py
python3 scripts/test_language_learning_poml.py \
  --cli /workspace/poml/python/poml/js/cli.js \
  --context-db /workspace/release/data/praxis.db \
  --template /workspace/release/templates/language_learning.poml
```

These checks cover TLS configuration/idempotency, local HTTP STT/TTS contracts and error handling, TTS parameter serialization/ranges, PCM/WAV handling, wake words/voice buffering, multi-turn gateway-response matching over a local WebSocket, Ollama stream correctness, and POML rendering against the saved context. They do not establish real key validity, speech-recognition quality, Discord network transport, or audible playback. The live acceptance test above remains necessary.
