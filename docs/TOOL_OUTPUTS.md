# Model-controlled tool responses

**No `_output` parameter means the FULL response is sent to the LLM.** There is
no implicit 2,000-character preview and no implicit 32,000-character page. The
old persisted `settings.tool_result_limit` remains readable for compatibility
but no longer clips model-facing tool results. Dashboard/activity previews are
separate presentation, not the model's evidence.

The same optional gateway parameter is advertised on builtin and plugin tool
schemas in the agent, ordinary-message and standalone tool loops. `_output` is
reserved for Praxis; it is validated and removed before invoking the actual
plugin/tool. Invalid controls fail before side effects.

## Examples: arguments the model chooses

Full terminal response, including stdout, stderr and exit code:

```json
{"command":"git status --short"}
```

Only the last 40 stdout lines:

```json
{"command":"cargo test --locked","_output":{"view":"tail","line_count":40,"json_pointer":"/stdout"}}
```

A specific range from a file response:

```json
{"path":"/workspace/report.txt","_output":{"view":"lines","start_line":100,"line_count":40}}
```

Literal, case-sensitive search (not regex), optionally selecting a JSON field:

```json
{"command":"some-authorized-command","_output":{"view":"search","query":"ERROR","json_pointer":"/stderr","line_count":20}}
```

Explicit paging, only when the model requests a bound:

```json
{"path":"/workspace/report.txt","_output":{"view":"full","max_chars":12000}}
```

Available controls:

- `view`: `full` (default), `lines`, `tail`, `search`.
- `start_line`: 1-based, defaults to 1; used for lines/search.
- `line_count`: 1–2000, defaults to 100; maximum lines or search matches.
- `query`: required only for search, 1–256 characters.
- `json_pointer`: optional RFC 6901 pointer applied **before** the view, e.g.
  `/stdout`, `/stderr`, `/items/0`. Strings become their text; other JSON values
  remain JSON. Missing/invalid fields return an explicit selection error.
- `max_chars`: **optional** 1–32000 Unicode characters. Omit it to receive the
  whole selected view. It is a paging preference, not the default response cap.
- `offset`: Unicode character offset within that exact selected view, default 0.
  Prefer following the returned `next` arguments rather than guessing offsets.

## Inspect again without execution

Responses include an `output_id`. Normal JSON objects keep their original
fields and add `_praxis_tool_output` metadata; JSON arrays/scalars (or objects
already using that reserved metadata name) are wrapped under `result`.
Text responses carry a separate reference footer. The saved snapshot always
contains the original returned text, not this delivery metadata.

Use `read_tool_result` with `output_id` and the view controls **directly**, not
nested in `_output`:

```json
{"output_id":"out_0123456789abcdef0123456789abcdef","view":"lines","start_line":100,"line_count":40}
```

Only `output_id` returns the full saved response. If an explicit page limit or
search match limit leaves more, pass the returned `next` object unchanged to
`read_tool_result`. This never re-executes the command, plugin, write or other
original action. A selection error after successful execution still returns the
reference and says the tool already executed; do not retry the action itself.

Image content parts are preserved separately. This text reader is not an image
cropper, media decoder or a way to retrieve arbitrary host files.

## Limits, durability and privacy

- Snapshots survive process restart and prompt-history compaction. They are
  readable only for the active authenticated user/session, not by guessed IDs
  from another user/session. Disabling `read_tool_result` remains effective.
- Up to 8 MiB of UTF-8 response text per snapshot. Exceeding that safety limit
  is explicitly marked `storage_truncated`; missing bytes cannot be recovered.
- Retention: at most 7 days, 200 responses / 64 MiB per user across sessions,
  and 2000 responses / 512 MiB globally. Oldest entries are evicted first.
  These bounds describe retained content, not SQLite/WAL physical file size.
- Actual history/session deletion removes its snapshots; ordinary visual chat
  hiding does not promise deletion. Context deletion removes all owner snapshots.
- Foreground host terminal capture drains stdout/stderr concurrently and keeps
  up to 1 MiB per stream, explicitly reporting `stdout_truncated` and
  `stderr_truncated`. A full saved response is not an unlimited process log.
  Background/VM/plugin tools can have their own source limits; a cached reader
  cannot invent data those tools never returned. `read_file` returns full regular
  UTF-8 files up to 8 MiB, and explicitly refuses oversized captures rather than
  silently returning the old 10,000-character prefix.
- Results live in the same SQLite database as chat, not in public temporary files.
  They can contain sensitive tool output: protect the data directory and backups.
  This is application-level scoping, **not** a new shell/filesystem sandbox or
  automatic redaction/encryption of all tool-returned data.
- Full output can consume substantial model context. The model should choose
  `_output` deliberately for large results. Model/provider context limits still
  exist; no hidden clipping is used to pretend a complete response was read.
- Historic text already discarded by older Praxis versions cannot be restored.

Offline tests use temporary databases/files, a scripted LLM, real local POML
rendering and harmless local shell commands. They verify default-full delivery
(over 32,000 characters despite a legacy limit of 1), explicit selection and
subsequent full rereading across all three tool loops, with exactly one original
execution. No GPU inference, live credentials, deployment or rental is required.
