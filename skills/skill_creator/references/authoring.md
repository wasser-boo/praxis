# Native Praxis skill authoring

Use this reference only while creating/editing a skill. POML syntax is deliberately
not duplicated: load `poml_templates` with `target_kind: "skill"` for that work.

## Package contract

```text
skills/<folder>/skill.json
skills/<folder>/skill.poml
skills/<folder>/references/...     # optional, read only when needed
skills/<folder>/scripts/...        # optional, never autorun by the loader
```

The loader supplies the absolute `skill_dir` variable and prints the directory in
the tool result. Resolve every relative helper/reference against that directory,
not an assumed shell cwd. The installation root is its `skills/` ancestor.

```json
{
  "name": "my_skill",
  "description": "Specific tasks and search keywords, including the user's language.",
  "required_parameters": ["user_request"],
  "skill_hidden": false,
  "user_only": false
}
```

- Name: 1–64 ASCII letters/digits/underscore/hyphen, unique across all folders.
  Avoid Discord control names `list`, `search` and `off`.
- Description: 1–1024 characters. This is metadata, **not the workflow body**.
  Search results show at most 320 characters. Put useful trigger words early.
- Manifest: at most 16 KiB. At most 16 required string inputs, names at most 64
  characters. Older manifests default to an empty input list and false flags.
- `skill_hidden`: omit from model discovery (including POML-pinned candidates).
  An ordinary hidden skill can still be loaded as an exact-name dependency.
- `user_only`: `use_skill` and agent-originated activation reject it, even when
  hidden=false. Human discovery shows it; the model may ask the user to select it.
- Automatic human selection maps the latest task to `user_request`, `code`,
  `error`; other required inputs must be provided explicitly. Do not require
  unrelated context or credentials in skill input.
- A manifest and main POML file must be regular local files, not symlinks.
  Grouping/sharded folders are supported by the index (up to 16 directory levels).
  Do not nest another skill inside a skill package's references or scripts.

## Progressive disclosure

Default discovery does **not** put the whole catalog into the prompt, even as
names/descriptions. `templates/discovery/skills.poml` defines the strategy and can
choose a few metadata candidates from names or index queries; `search_skills`
returns another bounded set on demand. Only `use_skill` or explicit human
activation loads instructions. References stay separate unless a workflow needs
them. Do not eagerly import a whole reference library via POML `let src`.

## Validation and publication

Use a synthetic context such as `{"user_request":"Validate this skill"}`. Validate
the staged POML with the **existing** `poml_templates/scripts/validate.py`; it
calls Microsoft's real CLI without any LLM/provider requests. Skills are trusted
local prompt code, so review imports before rendering and never import secrets.
For an existing package preserve a backup, validate a complete staged copy with
correct relative imports, then publish the intended changes. Skill loading does
not validate executable scripts' behavior; test helpers separately with mocks.

From the installation working directory, refresh just one folder:

```bash
./praxis skill index --folder my_skill
# For grouped folders:
./praxis skill index --folder documents/my_skill
```

This offline command does not load `.env`, unlock secrets or start services. Its
index storage defaults to `./data`. If the service uses another `DATA_DIR`, pass
`--data-dir /actual/data/path`; use `--directory /installation` from another cwd.
A full operator rebuild is `./praxis skill index`; do it after bulk imports or
removals, not after every model turn. Duplicate names fail rather than overriding
an existing activation policy. Index creation/refresh is metadata-only and is
not evidence that POML renders or that a helper ran.

Human activation:

```text
/skill skillname:my_skill
/skill skillname:off
```

Non-user-only task-local loading:

```json
{"name":"my_skill","parameters":{"user_request":"The actual task"}}
```

Never claim a skill was activated by merely writing/indexing files. Unrestricted
local file/terminal access is not a security sandbox; do not use it to bypass
manifest flags or human activation controls.
