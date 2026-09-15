# ComfyUI workflow plugin for Praxis

Build **complete connected workflows**, edit existing ones, upload arbitrary local
inputs, run a prompt, and download workflows/results through ComfyUI's HTTP API.
Python **3.9+**, standard library only. Uses Praxis's existing plugin/tool command
handling and `/api/files` download endpoint. No new Discord, attachment, command
framework, gateway routes, database, or TTS backend.

## Install and configure

Copy this **whole folder**, including helpers and examples:

```sh
praxis plugin install /path/to/praxis-source/plugins/comfyui
praxis plugin list
```

In an existing source checkout the plugin is already in `plugins/`. Restart Praxis
in a controlled manner to load it. `repair-assets` does not install this optional
plugin. No running service, model, GPU node, or configuration is changed merely by
adding these files.

Set the endpoint in the **Praxis process**, not on the GPU:

```dotenv
COMFYUI_BASE_URL=http://100.80.1.2:8188
# Optional reverse-proxy bearer credential, NOT a Comfy Platform billing key:
# COMFYUI_API_KEY=...
# Optional existing ComfyUI multi-user profile ID (Comfy-User header):
# COMFYUI_USER=...
COMFYUI_WORKFLOW_TIMEOUT_SECONDS=900
COMFYUI_MAX_FILE_BYTES=67108864
```

Replace the example private IP with your server. HTTPS endpoints may include a
reverse-proxy path prefix. Plain HTTP is limited to literal RFC1918/CGNAT/ULA IPs,
loopback or `localhost`; use HTTPS for DNS names/public addresses. Credentials,
query strings and fragments are not allowed in the base URL. Proxies from the
process environment and HTTP redirects are disabled. Do not expose an unprotected
ComfyUI instance publicly; use private networking/ACLs or an authenticated HTTPS
proxy.

Per-user plugin overrides use existing `/context` handling:

```text
/context set custom_data.comfyui_workflow_base_url=http://100.80.1.2:8188
/context set custom_data.comfyui_workflow_user=YOUR_EXISTING_COMFY_USER_ID
```

Empty/null plugin context fields inherit the environment. These are **plugin**
settings; native TTS `settings.comfyui_*` fields are not automatically passed to
script plugins. Both integrations can share `COMFYUI_BASE_URL`, but this plugin
never changes automatic speech settings. Store an optional bearer token in Praxis
custom secrets as `comfyui_api_key` (preferred over `COMFYUI_API_KEY`), never in a
workflow, tool argument, POML or context. The profile header is not authentication.
A stock private ComfyUI server normally needs no API key.

## Existing tool commands

Discover with Praxis's `search_tools` query `comfyui`. These commands become callable
through the normal agent/chat plugin registry:

| Command | Purpose |
|---|---|
| `comfyui_nodes` | Search installed node classes; inspect exact input schemas, model/file choices and output slots |
| `comfyui_workflow` | `list`, `get`, `save` (create/explicit replacement), `edit` (JSON Patch), `validate` |
| `comfyui_run` | Queue an API graph once with prompt, file and typed parameter bindings |
| `comfyui_result` | Check/wait for the same prompt ID and download completed output files |

The complete argument schemas are in `plugin.json`. There is intentionally no new
Discord slash command or `praxis comfyui` CLI parser. Scripts also follow the normal
plugin environment protocol, e.g. `PLUGIN_ARGS='{"query":"sampler"}' python3
plugins/comfyui/nodes.py` from the installation directory with the endpoint exported.

## Creating a working workflow

1. Search `comfyui_nodes` for the needed loaders, encoders, samplers and output nodes.
2. Inspect **each exact class**, e.g. `{"node_class":"CheckpointLoaderSimple"}`.
   Use its actual model filenames, input names, types and output-slot indexes.
3. Construct API JSON: `{ "node_id": {"class_type": "...", "inputs": {...}} }`.
   Connections are `["source_node_id", output_slot_index]`. Include all required
   inputs and at least one output node such as `SaveImage`/`SaveAudio`.
4. Use `comfyui_workflow` with `action:"validate"` and one of `workflow`,
   `workflow_path`, or `workflow_name`. This checks installed classes, required
   inputs, choice values, basic types/ranges, missing links, output slots/types,
   cycles and an output node **without executing**.
5. `save` the graph, then run it only with user authorization. Only a successful
   `comfyui_result` confirms execution; inspect the generated artifact for quality.

`examples/txt2img.api.json` and `examples/img2img.api.json` contain complete stock
checkpoint/CLIP/VAE/KSampler pipelines. **Select an installed compatible Stable
Diffusion checkpoint first**: their checkpoint string is intentionally a placeholder,
not a claim that a model exists. The image-to-image template also needs a file
binding. These are not universal Flux/video/audio graphs; discover those models'
actual nodes and build the appropriate pipeline instead. Large source images may
need resize nodes to fit GPU memory.

ComfyUI performs the definitive `/prompt` validation and custom `VALIDATE_INPUTS`.
A metadata check cannot prove weights load, third-party Python works, enough VRAM is
available, or an API node has billing credentials. Custom dynamic schemas may not
be expressible through `/object_info`; for a reviewed workflow set `preflight:false`
on `comfyui_run` to defer schema checks to ComfyUI. Local format/binding/file safety
checks still apply. No nodes, models or credentials are automatically installed.

### Create/import and download

Example tool arguments (substitute paths/models with real ones):

```json
{
  "action": "save",
  "name": "portraits/edit.api.json",
  "workflow_path": "/path/to/a/configured-img2img.api.json"
}
```

Or supply the complete JSON graph as `workflow` instead of `workflow_path`.
`save` refuses an existing name unless `overwrite:true` is explicitly supplied.
The server location is `workflows/portraits/edit.api.json` in the selected ComfyUI
profile. `list` returns names relative to `workflows/`, including subdirectories.

```json
{"action":"get","name":"portraits/edit.api.json"}
```

Returns the graph, its server-content `sha256`, and `file.path`/`file.download_url`.
`include_workflow:false` returns only metadata and a downloadable copy. Downloads
work through the existing authenticated Praxis dashboard, not a new public route.

### Edit nodes, parameters, links or layout

First `get` the latest graph, then:

```json
{
  "action": "edit",
  "name": "portraits/edit.api.json",
  "expected_sha256": "REPLACE_WITH_TOP_LEVEL_SHA256_FROM_GET",
  "patch": [
    {"op":"replace","path":"/3/inputs/steps","value":30},
    {"op":"replace","path":"/3/inputs/denoise","value":0.5},
    {"op":"replace","path":"/6/inputs/text","value":"A watercolor portrait"},
    {"op":"replace","path":"/9/inputs/images","value":["8",0]}
  ]
}
```

`add /12` adds a complete node object; `remove /12` removes it. Edit connections
and required inputs accordingly, then validate. Editor JSON can be patched at
paths such as `/nodes/0/widgets_values/0` or `/nodes/0/pos`. Supports JSON Patch
`add`, `replace`, `remove`, `test`, including array indexes/`-` and `~0`/`~1` escapes.
A failed patch makes **no server write**. The top-level SHA refers to original
server bytes; the downloaded, reserialized file's SHA may differ.

The SHA detects a stale read but is **not a server-side compare-and-swap**. ComfyUI's
userdata API has no conditional-update guarantee; avoid concurrent edits to the
same workflow. Saving/editing checks JSON structure, not execution readiness, and
never spends GPU time. Validate separately after structural changes.

## Run a prompt with any supported local input

For the stock image-to-image example (node 6 text, node 10 image):

```json
{
  "workflow_name": "portraits/edit.api.json",
  "prompt": "Turn this into a watercolor painting at sunset",
  "prompt_targets": [{"node_id":"6","input":"text"}],
  "files": [{"node_id":"10","input":"image","path":"/srv/pictures/source.png"}],
  "overrides": [
    {"node_id":"3","input":"seed","value":12345},
    {"node_id":"3","input":"denoise","value":0.65}
  ],
  "wait_seconds": 0
}
```

- Instead of `workflow_name`, use `workflow_path` (host JSON) or `workflow` (inline
  API graph). Supply exactly one source.
- `prompt` requires explicit `prompt_targets`. Multiple targets are supported.
  Positive/negative prompts are never inferred from arbitrary node ordering.
- `overrides` preserve JSON types: numbers, booleans, strings, connections, objects
  or lists supported by the selected node. Set negative prompts, models, sizes,
  seeds or already-existing **ComfyUI input-relative filenames** here.
- `files` takes any readable regular file: images, masks, audio, video, 3D data,
  documents or other formats **if the installed loader accepts them**. No extension
  allowlist, Discord constraint, conversion or media decoding is applied. Files
  are sent as multipart bytes via `/upload/image` (the upstream route is a generic
  file writer despite its name). The server may impose its own body-size limits.
- Each upload gets a unique server name; the returned `subfolder/name` replaces
  exactly the chosen input. Multiple files/nodes work; the same canonical local
  file is uploaded once per run. Saved templates and source files remain unchanged.
- Paths resolve on the **Praxis host**, not on the GPU. Absolute paths, `~`, and paths
  relative to the Praxis process cwd work. Existing `/mnt/shared/...` references
  map to `$DATA_DIR/shared/...`; existing `/api/files/<name>` references map to
  `$DATA_DIR/uploads/<name>`. In explicit VM-only mode, inputs are limited to the
  host shared directory. No new attachment downloader/history search is added.
- Inputs must already exist locally; URLs and inline data/base64 URLs are not
  downloaded. An earlier downloaded file is just another existing path. There is
  no basename guessing or automatic search of someone else's files.
- A node that expects a custom absolute GPU path/API rather than an input-relative
  filename may need its own server-side setup; no universal loader is invented.

The response contains `prompt_id`, uploaded-file mappings and a downloadable
snapshot of the actual submitted graph. Default `wait_seconds:0` returns as soon
as ComfyUI acknowledges the queue. To wait/download later:

```json
{"prompt_id":"THE_RETURNED_ID","wait_seconds":30}
```

Call **`comfyui_result`**, not `comfyui_run` again. Completed results include local
paths, `/api/files` download URLs, sizes/hashes and original descriptors under any
output key (images, audio, video/gifs or custom files). Structured/text outputs are
returned inline up to 32 KiB, otherwise as a downloadable JSON file. Custom nodes
must publish standard `filename/subfolder/type` descriptors for `/view` downloads;
returned URLs/absolute paths are never followed. File extensions/MIME metadata are
not proof of content validity. Nothing is played, opened or sent automatically.

`output_nodes` narrows which results are downloaded, **not which nodes execute**.
Absent history means pending/unknown, not success: the job may still be running,
or ComfyUI may have restarted/pruned its in-memory history.

## Limits, failures and security

- Default per-file limit: 64 MiB; `COMFYUI_MAX_FILE_BYTES` permits 1 byte..1 GiB.
  At most 32 file bindings/output descriptors and four times the per-file limit
  per input/output batch. Increase only with adequate RAM and matching server
  limits; uploads/downloads are bounded in-memory, not unbounded streams.
- Workflow JSON: 4 MiB/2000 API nodes; API responses: 16 MiB; patches: 100 operations.
- `COMFYUI_WORKFLOW_TIMEOUT_SECONDS`: 0.1..3600 seconds, default 900. Each operation
  shares one wall-clock budget across requests/polling/downloads. `wait_seconds`
  is 0..900. Network workers cannot publish late files after timeout.
- Exactly one queue POST, no automatic retries, redirects, fallback, `/interrupt`,
  queue clearing or history deletion. Timeout/lost acknowledgement may leave a
  job running; known prompt IDs are preserved in errors. Inspect history before
  retrying. A lost POST acknowledgement may leave the ID unknown.
- Uploads are server side effects even if a later upload/validation fails. Operators
  manage server input/output/history retention. The plugin never deletes remote
  files. Local output batches use random names, exclusive atomic publication and
  cleanup on ordinary errors; hard process/machine crashes can leave staging files.
- Plugins are trusted local code, not a multi-tenant sandbox. Only run reviewed
  workflows against trusted endpoints, using files the user authorized. ComfyUI
  custom nodes may execute arbitrary code or paid services. Server error bodies
  are not echoed; validation reports bounded node IDs and error-type codes.

## Research: actual upstream API contracts

Checked against official documentation and upstream code (not invented endpoints):

| Operation | ComfyUI endpoint |
|---|---|
| Available node classes, widget/model choices, output types | `GET /object_info`, `GET /object_info/{node_class}` |
| List saved workflows | `GET /userdata?dir=workflows&recurse=true&split=false` |
| Read/create/replace workflow JSON | `GET/POST /userdata/{URL-encoded-file}`; `overwrite=false` guards creation |
| Upload an input file | `POST /upload/image`, multipart field `image`, `type=input`, optional `subfolder` |
| Submit the executable graph | `POST /prompt` with `{"prompt": API_GRAPH, "client_id": "..."}` |
| Check a specific run | `GET /history/{prompt_id}` |
| Download a file descriptor | `GET /view?filename=...&subfolder=...&type=...` |

Sources:
- [Official server routes](https://docs.comfy.org/development/comfyui-server/comms_routes)
- [Upstream server.py: uploads, nodes, prompt, history and view](https://github.com/Comfy-Org/ComfyUI/blob/master/server.py)
- [Upstream user_manager.py: userdata and Comfy-User](https://github.com/Comfy-Org/ComfyUI/blob/master/app/user_manager.py)
- [Official API graph example and Export (API) instructions](https://github.com/Comfy-Org/ComfyUI/blob/master/script_examples/basic_api_example.py)

**Editor and executable formats differ.** Both are JSON and can be saved/downloaded
via userdata. The editor's `nodes`/`links` format contains UI widgets/layout; `/prompt`
expects `{node_id:{class_type,inputs}}`. The stock API does not provide a universal
editor-to-executable conversion or an endpoint that turns natural language into a
workflow. Praxis constructs the graph from discovered schemas; an existing editor
workflow should be exported using **File > Export (API)**. Browser-only unsaved
workflows must be saved/exported first. This plugin targets the self-hosted ComfyUI
API, not Comfy Cloud or a custom hosted provider protocol.

## Offline verification

From the Praxis source distribution:

```sh
python3 scripts/test_comfyui_plugin.py
cargo test --locked --lib comfyui_plugin -- --test-threads=1
```

Tests cover full sample graph connections, create/edit/export, schema validation,
input binding/upload, queued/pending/failed runs, output downloads, hostile paths,
limits/timeouts/redirects, and the existing Praxis registry/secret dispatch. They
use local mock servers only. No real GPU/model execution or quality claim is made.
For a live smoke test, configure your endpoint, select installed compatible models,
validate a small workflow, explicitly run it, fetch its prompt ID, and inspect the
artifact. Live execution has not been verified merely by these offline tests.
