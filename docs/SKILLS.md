# Praxis skills and POML authoring

Praxis loads native skills from its **working directory**: `skills/<folder>/skill.json` plus `skill.poml`. This is not Pi's `SKILL.md` format. No external skill installer is needed. The registry is loaded when building prompts and when calling a skill; malformed manifests are logged and skipped so other skills remain available.

## Use a skill

### Persistent Discord selection

For your paired Praxis context, use `/skill` with the **skillname** option:

```text
/skill skillname:poml_templates
/skill skillname:code_review
/skill skillname:off
/skill
```

Omitting the option (or using `list`) lists registered skills and the current selection. A valid selection persists as `settings.active_skill` for **subsequent tasks**, until changed or switched off. Subsequent raw input is mapped to required `user_request`, `code` or `error` parameters. Loading instructions does not itself execute scripts, spend on a provider or enable tools. Pairing, guild/channel restrictions and disabled `use_skill` remain enforced; unavailable/invalid skills return errors. Custom templates cannot accidentally omit the active skill: the runtime appends its rendered instructions separately.

**Access rule:** every Discord `/skill` operation (including omitted argument, `list`, `off` and unknown names) requires a completed pairing for the interaction's Discord user ID. A pending `/pair` code or knowledge of an internal user/context ID is insufficient. Pairing and guild/channel checks happen before reading skill files and are repeated before returning skill data or saving the selection. Revoked pairings are denied on subsequent calls; failures do not change another user's context. Replies remain ephemeral. This rule is for the Discord command; the separate authenticated Gateway `use_skill` tool is unchanged.

The Discord command appears after the updated bot binary registers commands at startup. It can remain visible to unpaired users, but execution is rejected with a pairing hint. The source changes do not update a currently running bot.

### Per-task tool loading

In agent mode, ask Praxis to perform a matching task, for example:

> Use the poml_templates skill to create tasks/tutor with French practice, German explanations, and a safe optional user_prompt. Validate it, but don't activate it yet.

The actual tool call is:

```json
{
  "name": "poml_templates",
  "parameters": {"user_request": "Create a French tutor with German explanations"}
}
```

This is the **arguments object for `use_skill`**, not a direct shell command. The tool returns rendered instructions. The agent must then follow them using normal tools. Loading a skill does not itself write files, run scripts, change settings, or call another model.

| Skill | Required non-empty string parameter |
|---|---|
| `code_review` | `code` |
| `debug` | `error` |
| `tmux` | `user_request` |
| `poml_templates` | `user_request` |

Actual tmux operations require `tmux` to be installed on the host (it is not installed in this development environment). Wrapper tests use a stub, not a real server. The create wrapper preserves failure exit codes; the kill wrapper requires an exact target name.

Only the explicit `parameters` are supplied as top-level POML variables. Stored user context and secrets are **not** automatically merged into skill inputs. Unknown names, missing/wrong-type required inputs, disabled `use_skill`, missing renderer and invalid POML return errors. Skill names are resolved through the registry, not interpolated into a caller-controlled path.

Manifest example:

```json
{
  "name": "my_skill",
  "description": "Explain when to use this skill.",
  "required_parameters": ["user_request"]
}
```

Older manifests can omit `required_parameters` (defaults to `[]`). This field is a list of required string inputs, not a general JSON Schema. Each skill name should be unique. `skills` in the system render context lists `name`, `description` and `required_parameters` in name order. There is no automatic execution of `scripts/`.

## Renderer setup and compatibility

Install/build Microsoft's POML JavaScript CLI and Node, then configure:

```bash
export POML_CLI=/absolute/path/to/poml/js/cli.js
```

For this checkout the tested CLI is `/workspace/poml/python/poml/js/cli.js`. This path is local setup, not a bundled dependency. Strict operations require the variable to be set in the **Praxis service environment**, not just another shell.

The new skill follows [Microsoft's template-engine documentation](https://microsoft.github.io/poml/stable/language/template/) and includes tested examples of interpolation, `let`, JSON imports, `for`, `loop.index`, `if`, relative `include`, and absent/null inputs. Its reference is `skills/poml_templates/reference.md`.

Two tested CLI details matter:

- For literal imports, this installed version accepts `type="string"` and rejects the stable page's `type="text"`. JSON imports work with inferred types (or `object`/`array`). The skill documents the discrepancy rather than shipping a broken documentation example.
- With `--strict --speakerMode=false` and without `--prettyPrint`, stdout is a JSON envelope whose `messages` field is plain text. `render_strict` unwraps it and rejects empty/non-text results. It has a 30-second deadline and kills a timed-out child.

The older general `poml::render` helper still has a simple-substitution fallback for compatibility. It is **not validation**. Skill execution, shared user/system rendering and `update_template` use the strict, no-fallback path instead. Configure the real CLI before deploying the new default templates.

## Create, validate, then activate

The authoring skill guides the agent to call `update_template`, for example with arguments:

```json
{
  "name": "tasks/concise",
  "content": "<poml><task>Answer accurately and concisely.</task></poml>",
  "context": {}
}
```

- Names use letters, digits, `_`, `-` and `/` separators, without `.poml`. Absolute paths, traversal and symlinked destination subdirectories/files are rejected.
- `context`, when supplied, is the complete JSON object for validation. Omitting it uses synthetic system variables, never saved user data.
- A temporary sibling is rendered first, preserving relative include/import resolution. During validation an embedded Node read overlay redirects reads of the final destination to the candidate, so direct/transitive self-includes cannot accidentally validate against the old file. The overlay never writes the destination. Only a successful, nonempty render replaces the target. Failed rendering preserves an existing file and does not save a new one.
- After file persistence, the template is mirrored to the DB. This is not a cross-storage transaction; a DB failure is reported as such, with the valid file already saved.
- The agent must read an existing template before editing it and preserve unrelated changes. Saving does not activate a template. Only if requested, call `set_context` with `key: "settings.system_template"` and `value: "tasks/concise"`.
- Templates are trusted local prompt code, not sandboxed markup. Rendering has a time limit but no stdout/stderr byte limit. Authorize all imports; don't load secrets, remote code, or untrusted templates to validate them.

For additional local checks (from any cwd):

```bash
python3 /path/to/praxis/skills/poml_templates/scripts/validate.py \
  /path/to/praxis/templates/tasks/concise.poml \
  --cli /absolute/path/to/poml/js/cli.js
# Add --context-file /tmp/sample.json repeatedly for synthetic test cases.
```

The helper uses `{}` by default, prints the actual rendered prompt and fails on bad JSON, render failure, empty output, a missing CLI, or timeout. Validate missing/null/empty/populated optional values, both condition branches, empty/nonempty loops, and all includes. Never use private context dumps for test inputs.

## Regression tests

From the Praxis source root:

```bash
export POML_CLI=/absolute/path/to/poml/js/cli.js
python3 scripts/test_skills.py
python3 scripts/test_poml_templates.py
python3 scripts/check_context_docs.py
cargo test --offline --release --features songbird --lib skill -- --include-ignored --test-threads=1
cargo test --offline --release --features songbird --lib validated_template -- --include-ignored --test-threads=1
cargo test --offline --release --features songbird --lib db::tools::tool_tests
cargo test --offline --release --features songbird --lib gateway::poml::poml_tests
```

On memory-constrained machines add `--config 'profile.release.package.praxis.opt-level=0'` to each cargo command to leave dependencies cached/optimized while compiling only Praxis itself without optimization. This is the setting used for the development regression run; no project profile is changed.

The `--include-ignored` tests require the real Node/CLI installation; ordinary unit tests do not. Tests use synthetic contexts and temporary databases/files, never a live provider or terminal session. They verify both gateway dispatchers and the public LLM tool loop (with a mock provider), all shipped skills, registration/upgrade/disable behavior, parameter checks, invalid-manifest isolation, prospective include-cycle rejection, and preservation of old templates on failed renders. They do not prove model task quality or live-service deployment.

See [Semantic templates and workflows](POML_WORKFLOWS.md) for customizable profiles, full-context fixtures and canonical `sm_file` routing.

## Deployment

Interactive onboarding now installs the complete native skill directories, including POML references/examples/scripts. For an installation made with the incomplete older wizard, use an updated binary's `repair-assets --directory /path/to/install --update-dashboard` rather than repeating onboarding; this preserves configuration, memory and existing prompt/skill customizations. See [safe installation repair](POML_WORKFLOWS.md#repair-an-incomplete-onboarding-installation).

These changes are in the source checkout. Rebuild Praxis with the features you normally use, deploy the updated binary **and** the complete `skills/`, `templates/`, `contexts/` and `static/` assets, then restart the service normally. Startup's default-tool migration adds `use_skill` to existing installations while preserving disabled tools. A running old binary cannot acquire a new dispatcher merely by copying the skill directory. No live database, running binary, service configuration, or active user template is changed by this development/test workflow.
