# ComfyUI Qwen3-TTS and shared German/Japanese speech

`comfyui_qwen3` adds **Qwen3-TTS-12Hz-1.7B-Base** as an optional native TTS
backend. The older `qwen_tts` HTTP provider and `comfyui_xtts` remain available.
Qwen3 supports ten languages, including German and Japanese. Its `auto` setting
is not a guarantee of correct language selection for every mixed sentence.

## One context switch, one combined WAV

Both ComfyUI backends support the same typed setting:

```text
/context set settings.comfyui_tts_language_mode=de-ja
```

In this mode, Japanese scripts (including **kanji without kana**) are pronounced
in Japanese; other alphabetic text is pronounced in German. For example:

> Auf Japanisch heißt Schule 学校. Das liest man がっこう. Bitte wiederholen.

One ComfyUI job launches **one worker/model instance**, synthesizes the language
segments in their original order, and combines their samples into **one PCM16
mono WAV**, with 120 ms gaps. It does not concatenate WAV headers or return
multiple clips. Praxis downloads that one WAV and uses its existing persistence,
optional RVC, dashboard playback and Discord delivery. Failure of any segment
fails the whole reply; incomplete audio is not published.

Turn mixing off without losing the provider's ordinary language setting:

```text
/context set settings.comfyui_tts_language_mode=single
```

`single` means `comfyui_xtts_language` for XTTS and `comfyui_qwen_language` for
Qwen3 (default `auto`). Existing fixed-language XTTS uses its original synthesis
path. The mixed setting does not affect ElevenLabs, MiMo, MiniMax or the older
Qwen HTTP provider. It controls pronunciation, **not translation**.

**Romaji/Latin text cannot reliably identify Japanese.** In mixed mode, explicitly
mark it when necessary: `Bitte sage [[ja]]arigatou gozaimasu[[/ja]].`
`[[de]]...[[/de]]` is also supported. Markers are removed from speech; nested,
unclosed or mismatched markers fail rather than being guessed. Mark a whole
Japanese phrase if it includes Latin acronyms. Mixed mode is specifically DE/JA,
not a general-purpose detector for other language pairs. Pronunciation of
ambiguous kanji still depends on the word/context; kana can make a reading clear.

## Requirements before activation

These features need **both an updated Praxis binary and the updated server node
package** at [integrations/comfyui_audio](../integrations/comfyui_audio/README.md).
The old server's `PraxisXTTS` does not accept `de-ja`. Installing source files in
Praxis alone does not install a model or reload ComfyUI.

The package preserves the `PraxisXTTS` node ID and adds `PraxisQwen3TTS`. Qwen runs
in a **separate virtualenv** so its Transformers dependencies do not change XTTS
or ComfyUI. Provision the pinned public model explicitly first; Qwen inference is
offline-only and never downloads weights or uses a cloud TTS service.

Never restart services while jobs/VM work need saving. After backing up files,
activate the server package during a maintenance window and rebuild Praxis with
`--features songbird` if Discord voice is needed. `repair-assets` supplies missing
files but intentionally does **not** replace customized workflows. No restart,
model download, context activation or deployment is triggered by these files.

## Qwen3 configuration

Resolution is typed `settings.*` → matching legacy `custom_data.*` → process
environment → default. Null/empty strings inherit; other providers' reference
paths and language settings are not reinterpreted.

| Setting | Environment | Default |
|---|---|---|
| `comfyui_base_url` | `COMFYUI_BASE_URL` | required private literal origin |
| `comfyui_tts_workflow` | `COMFYUI_TTS_WORKFLOW` | `workflows/tts-qwen3-api.json` for Qwen3; `workflows/tts-api.json` for XTTS |
| `comfyui_tts_language_mode` | `COMFYUI_TTS_LANGUAGE_MODE` | `single`; alternatively `de-ja` for both |
| `comfyui_qwen_reference_audio` | `COMFYUI_QWEN_REFERENCE_AUDIO` | `reference.wav`, relative to the **server input directory** |
| `comfyui_qwen_reference_text` | `COMFYUI_QWEN_REFERENCE_TEXT` | empty: speaker embedding only; optional exact reference transcript improves cloning |
| `comfyui_qwen_language` | `COMFYUI_QWEN_LANGUAGE` | `auto`; overridden by `de-ja` mode |
| `comfyui_timeout_seconds` | `COMFYUI_TIMEOUT_SECONDS` | 900, integer 1–3600, including queue/execution/download |

Qwen3 language codes: `auto`, `en`, `zh`, `ja`, `ko`, `de`, `fr`, `ru`, `pt`, `es`,
`it`. Only the wire workflow uses `de-ja`; use the shared mode field to select it.
The optional reference transcript is private speech data, not a style instruction.
An empty transcript selects `x_vector_only_mode=True`, which requires no transcript
but can reduce cloning quality compared with a correct transcript.

After deployment, select Qwen3 in the **same chat context used for playback**:

```text
/context set settings.voice_tts_type=comfyui_qwen3 settings.comfyui_tts_workflow=workflows/tts-qwen3-api.json settings.comfyui_qwen_reference_audio=reference.wav settings.comfyui_qwen_language=auto settings.comfyui_tts_language_mode=de-ja settings.use_tts=true settings.web_chat_tts=true
```

Set `settings.comfyui_base_url` to your GPU NetBird origin if not already configured.
The reference above means `/workspace/input/reference.wav` on the template's
GPU server. No new upload is needed if that file is already there. Only use a
reference voice for which you have permission. Qwen accepts a mono/stereo
reference of at most 120 seconds and the node enforces a 20 MiB file limit.

Or keep XTTS and enable mixing:

```text
/context set settings.voice_tts_type=comfyui_xtts settings.comfyui_tts_workflow=workflows/tts-api.json settings.comfyui_xtts_reference_audio=reference.wav settings.comfyui_xtts_language=de settings.comfyui_tts_language_mode=de-ja settings.use_tts=true settings.web_chat_tts=true
```

Run this **separately** to confirm the saved setting:

```text
/context get settings.comfyui_tts_language_mode
```

## German/French and other Qwen multilingual replies

Do **not** use `de-ja` for French: it deliberately routes Latin-script text to
German. Keep the same native Qwen provider, API workflow and authorized reference,
but select Qwen's own multilingual inference:

```text
/context set settings.comfyui_tts_language_mode=single settings.comfyui_qwen_language=auto
```

Here `single` disables the explicit DE/JA router; it does not restrict Qwen Auto
to one spoken language. For strictly French output, use `comfyui_qwen_language=fr`
instead. Auto is not a deterministic DE/FR segment classifier: listen to ambiguous
phrases before relying on their pronunciation. No extra `qwen_tts_model` field is
required by `comfyui_qwen3`: the server node loads its provisioned Base model.
`qwen_tts_model` belongs to the separate HTTP provider.

Speech recognition is configured independently; see [VOSK_REMOTE.md](VOSK_REMOTE.md).

## Workflow errors and limits

Workflow files are **local to Praxis** and must be bare API graphs, not editor
JSON containing `nodes`, `links`, `version`, etc. Qwen requires node `1` of class
`PraxisQwen3TTS` with `text`, `language`, `reference_audio`, `reference_text`.
XTTS retains node `1` of class `PraxisXTTS`. The adapter replaces these values from
the request/settings, so changing only the template's sample language has no effect.
An explicitly selected XTTS/GUI file is not silently replaced when changing provider.

Text is limited to 5000 characters and mixed speech to 64 segments. Long segments
are split at sentence/word boundaries where possible (Japanese ≤70 characters,
other segments ≤200). Combined speech is bounded to 600 seconds and 32 MiB.
The worker has a 600-second execution deadline; Praxis has a separate total budget
that includes queue time. Long replies can exceed either budget. There are no
automatic retries, partial-audio fallbacks, remote interrupts or queue deletions.
Both models are loaded once per job, not kept resident by a new service.

XTTS's existing license requirement/cache handling is preserved. The Qwen worker
only releases its own GPU allocations on exit. Neither coordinates Ollama VRAM.
A large Ollama context plus concurrent speech/image generation can still exhaust
VRAM; test actual workloads instead of assuming that 48 GB guarantees capacity.

## Verification

```sh
python3 -m unittest discover -s tests -p test_comfyui_audio.py
cargo test --locked --lib comfyui
cargo test --locked --lib message_audio_tests
cargo check --locked --features songbird --tests
python3 scripts/check_context_docs.py
```

These are offline contract tests, **not a claim that speech sounds natural**.
After authorized deployment, synthesize a short DE/JA sentence on each backend,
verify a single downloadable WAV and native replay, and listen to the language
switches. Kanji pronunciation, cloning similarity, latency and concurrent VRAM
use remain live acceptance tests.

Official references:
- https://github.com/QwenLM/Qwen3-TTS
- https://huggingface.co/Qwen/Qwen3-TTS-12Hz-1.7B-Base (Apache-2.0)
- Existing transport/security contract: [COMFYUI.md](COMFYUI.md)
