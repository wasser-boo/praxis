# ElevenLabs TTS plugin

Install this complete folder with `praxis plugin install /path/to/elevenlabs_tts` and restart the gateway when appropriate. Requires Python 3.9+ (`python3`), no pip packages.

Tool: **`elevenlabs_tts`**. Supply `text` (max 5000 characters) and an ElevenLabs `voice_id`, or set `custom_data.elevenlabs_tts_voice_id` / `ELEVENLABS_VOICE_ID`. The default model is `eleven_multilingual_v2`; override with `model_id`, `custom_data.elevenlabs_tts_model_id` or `ELEVENLABS_TTS_MODEL`.

Credential: Praxis secret `elevenlabs_api_key`, or `ELEVENLABS_API_KEY`. The updated gateway reuses the native secret and allows an explicit Custom Secret override. Do not put keys in tool arguments or context. Keep `media_common.py` alongside `generate.py`.

Output: unique MP3 in `$DATA_DIR/uploads`, with absolute `path`, authenticated dashboard `download_url`, MIME type and size. No autoplay, translation, cloning or audio/base64 in tool results. `language_code` is not supported by the default multilingual-v2 model; omit it for automatic language or explicitly choose a supporting model.

**Paid calls.** One POST per tool invocation, no automatic retries/redirects. HTTP budget 120 s (`ELEVENLABS_TTS_TIMEOUT_SECONDS`, max 600 s), output cap 16 MiB. Endpoint override for operators: `ELEVENLABS_API_BASE` (HTTPS, except loopback test servers). Existing files are never overwritten.

Full usage, optional voice settings, safeguards and offline tests: `docs/MEDIA_PLUGINS.md` in the Praxis source distribution. This plugin does not change Praxis's automatic voice/TTS settings.
