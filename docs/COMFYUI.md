# ComfyUI XTTS-v2 backend

Praxis stays local; ComfyUI runs on the GPU server over NetBird at
`http://<GPU_NETBIRD_IP>:8188`. Select **`comfyui_xtts`** as the native TTS backend.
It uses `PraxisXTTS` through `/prompt` → `/history/{prompt_id}` → `/view`, downloads
a WAV locally, and returns its bytes to the existing RVC, audio persistence,
dashboard and Discord playback pipeline. There is no separate XTTS service.
Existing providers and delivery systems remain unchanged; no automatic fallback.

**German/Japanese within one reply:** the optional
`settings.comfyui_tts_language_mode=de-ja` mode combines the ordered language
segments into one WAV. It requires the updated server node package. The same
setting also works with the optional `comfyui_qwen3` backend; see
[Qwen3 and shared mixed speech](COMFYUI_QWEN3.md) for installation and examples.
`single` (default) keeps the previous fixed-language XTTS behavior.

**Image integration is intentionally excluded.** Use your image plugin separately.
This change adds no image adapter, image workflow, or plugin registry changes.

## Installation and configuration

Build the updated Praxis binary (include `--features songbird` for Discord voice).
Run `./praxis repair-assets` in the target installation to supply the missing
`workflows/tts-api.json` without replacing existing edits, or copy it from this
checkout. Native XTTS needs no plugin installation. This change does not deploy to
`/workspace/release`, restart services, alter GPU files, or contact the GPU itself.

Use the following typed per-user/session **`settings.*` fields** via `/context`
or the dashboard's context settings editor. Environment variables in the **local
Praxis process** provide installation-wide defaults. Resolution:
`settings.*` → legacy `custom_data.*` → environment → default.
Null/empty strings inherit; a null timeout inherits, while an explicit timeout
must be an integer from 1 to 3600. These fields are saved in the context database.
The error for an editor/GUI JSON now includes the exact local workflow path;
`nodes`/`links` editor exports are not API graphs.

Earlier `custom_data.comfyui_*` values remain compatible, but new configuration
should use `settings.comfyui_*`. To stop inheriting an old value, copy it into
settings if wanted, then remove that legacy custom-data key. Existing provider
settings and unrelated custom data are unchanged.

| Context key (under `settings`) | Environment variable | Default / meaning |
|---|---|---|
| `comfyui_base_url` | `COMFYUI_BASE_URL` | Required; GPU NetBird origin, e.g. `http://100.80.1.2:8188` |
| `comfyui_tts_workflow` | `COMFYUI_TTS_WORKFLOW` | `workflows/tts-api.json`; **local Praxis** file |
| `comfyui_xtts_reference_audio` | `COMFYUI_XTTS_REFERENCE_AUDIO` | `reference.wav`; relative to **server** input directory |
| `comfyui_xtts_language` | `COMFYUI_XTTS_LANGUAGE` | `en`; XTTS pronunciation code, not translation |
| `comfyui_tts_language_mode` | `COMFYUI_TTS_LANGUAGE_MODE` | `single` (provider language) or `de-ja` (one mixed German/Japanese WAV); requires updated server nodes |
| `comfyui_timeout_seconds` | `COMFYUI_TIMEOUT_SECONDS` | `900`; integer 1–3600; submission + queue + execution + download budget |

Relative workflow paths resolve from the Praxis working directory; absolute local
workflow paths are supported. The template must be API format, retaining node
`1` of class `PraxisXTTS` with `text`, `language`, `reference_audio` inputs. The
adapter populates these inputs and reads `outputs["1"].audio`, not a server path.

Example local process environment (replace the address):

```dotenv
COMFYUI_BASE_URL=http://100.80.1.2:8188
COMFYUI_TIMEOUT_SECONDS=900
```

Select speech per context:

```text
/context set settings.voice_tts_type=comfyui_xtts settings.use_tts=true
/context set settings.comfyui_base_url=http://100.80.1.2:8188
/context set settings.comfyui_xtts_language=de
/context set settings.comfyui_xtts_reference_audio=voices/my-reference.wav
/context set settings.comfyui_timeout_seconds=900
```

For web speech also set `settings.web_chat_tts=true`; existing web OFF semantics
remain unchanged. `voice_tts_enabled` alone does not enable replies. Use existing
Discord `/join`, pairing and playback settings. Optional
`settings.voice_audio_output_path` copies and RVC use the existing pipeline.
Previously saved ElevenLabs/Qwen/MiMo language and voice settings are neither
cleared nor reinterpreted. Configure a separate XTTS reference/language per user.

XTTS codes: `en`, `es`, `fr`, `de`, `it`, `pt`, `pl`, `tr`, `ru`, `nl`, `cs`, `ar`,
`zh-cn`, `hu`, `ko`, `ja`, `hi`. Text is unchanged, limited to 1–5000 characters.
Provision an authorized WAV/MP3/FLAC/OGG reference via SCP or `docker cp` into
ComfyUI's **server** input directory (deployment: `/workspace/input`).
`voices/my-reference.wav` means `/workspace/input/voices/my-reference.wav` there,
not a local Praxis file. No reference-upload endpoint is assumed. The server
validates existence and its 20 MiB limit.

Review XTTS terms yourself; only if accepted, configure `COQUI_TOS_AGREED=1`
on the **GPU server** as described in `/workspace/vastai-template/DEPLOY.md`.
Praxis does not set it, download models, or accept licenses. Warm up XTTS on the
server first: initial model download can exceed the execution budget.

## Failure, cleanup and security

- One POST per invocation; no retries, redirects or provider fallback. Validation
  errors, execution errors/interruption, malformed history, missing/multiple
  outputs, empty/wrong-format WAVs and unsafe descriptors fail closed.
- Limits: 1 MiB workflow, 2 MiB per API JSON response, 32 MiB WAV including chunked
  downloads. RIFF/WAVE signatures and minimum size are checked, not audio quality.
  Connect timeout 10 s, API request timeout 30 s, download timeout 60 s, all bounded
  by the total execution budget.
- Temporary files have exclusive random local names; remote filenames never choose
  a local destination. Failed/cancelled downloads and files consumed as bytes are
  removed by ownership guards. Existing audio persistence owns long-term retention.
  Process/machine crashes may leave staging files for operator cleanup.
- The client/XTTS API accepts an optional cancellation token; cancellation/future
  drop stops local waiting/downloads and cleans staging. It **never** calls
  `/interrupt`, clears queues, deletes server outputs, or cancels another user's job.
- Native reply TTS retains existing detached-task behavior: successful conversation
  completion must not cancel its final speech. Web OFF suppresses late autoplay but
  retains generated audio for manual replay, as with the existing backends.
- Timeout/connection loss/cancellation can leave a remote job running. Errors/logs
  retain a known prompt ID; a lost POST acknowledgement leaves its ID unknown.
  Check server history before explicitly retrying. Operators manage orphaned jobs
  and server output retention; Praxis never blindly resubmits.
- Use NetBird ACLs allowing only the Praxis peer to TCP 8188. Do **not** publish
  ComfyUI ports or change listeners to `0.0.0.0`. The client accepts private literal
  IPs (CGNAT, RFC1918, IPv6 ULA; loopback for tests), not public IPs/DNS names,
  credentials, URL paths/query/fragments. This cannot prove routing: verify NetBird
  on the host. Environment HTTP proxies and redirects are disabled.
- Ollama stays separate, including GPU memory usage. ComfyUI's queue does **not**
  coordinate Ollama inference. No cross-service GPU lock is added. Serialize
  operationally when needed and validate VRAM before concurrent workloads.

## Verification gates

Offline (no GPU/model calls):

```sh
cargo test --locked --lib comfyui
cargo test --locked --lib message_audio_tests
python3 scripts/check_context_docs.py
```

Docker build/live GPU execution is **not verified by mocks**. Once available, run
from the Praxis machine:

```sh
cd /workspace/vastai-template
python3 scripts/smoke.py --url http://GPU_NETBIRD_IP:8188 --workflow workflows/tts-api.json
```

Then select `comfyui_xtts` in Praxis, join a Discord voice channel and request speech.
Listen to WAV/Discord output in required languages, verify per-user references and
dashboard replay, verify unauthorized/public access is blocked, and test VRAM
concurrency separately. These remain release gates until actually exercised.
