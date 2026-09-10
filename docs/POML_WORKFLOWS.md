# Semantic templates, skills and statemachines

## What selects the prompt

1. Load the current user's context and plugin defaults.
2. Write the **current raw input** to `custom_data.user_prompt`.
3. Select `settings.sm_file`, then root `sm_file`, otherwise `standard`.
4. Load the workflow, preferring `contexts/NAME.sm` over legacy `.cl`, and apply routing.
5. Strictly render `settings.system_template` (default `standard`) from `templates/`, with the shared runtime variables.
6. Append active skill instructions using this task's input. This does not execute a skill's scripts.

Both runtime paths use this preparation. Agent iterations reload persisted changes before subsequent rendering. Missing/malformed explicit workflows or templates are errors, not permission to use a different prompt silently. Preview shares the context builder and does not save its routing changes. Secrets are not part of the builder.

`sm_file` is canonical in saved/output JSON. Old `cl_file` and `settings.cl_file` inputs are normalized before merging; **`cl_data` is deliberately unchanged**. A settings-level workflow selection wins over the root-level one.

## Built-in profiles

| Template | Purpose |
|---|---|
| `standard` | General, helpful assistant; does not assume every task is programming |
| `language_instructor` | Configurable, gentle language practice with voice-friendly answers |
| `code_assistant` | Focused implementation, permission checks and verifiable tests |
| `researcher` | Evidence-based research with uncertainty and source awareness |
| `system`, `language_learning` | Compatibility aliases for `standard`, `language_instructor` |
| `roles/*`, `tasks/*` | Compatible role/task names rebuilt with valid POML and shared context |
| `user`, `compaction` | Current-message wrapper and evidence-preserving summary instructions |

The former prompts/workflows were preserved before replacement in `docs/archive/previous-prompts-and-workflows.zip`, outside template discovery. Existing template names remain usable; unsupported `<prompt>` components and unguarded optional variables were removed from active files.

### Switch persona

With the default `standard.sm` workflow, ordinary messages such as these trigger editable rules:

```text
Be a language instructor.
Teach me French.
Be a code assistant.
Act as a researcher.
Switch to standard.
```

A selected persona **sticks** for subsequent messages. A mere quoted mention does not match these anchored rules. This is deterministic routing, not a claim that regexes understand every paraphrase. Edit `contexts/standard.sm` to add variants or custom profiles. No persona regexes live in Rust.

To select explicitly, use the dashboard's dotted context editor or these context commands:

```text
/context set settings.sm_file=standard
/context set settings.system_template=language_instructor
```

In Discord `/context`, enter the text after `/context` in its **command** option. The `standard` router has no unconditional template assignment, so manual selections persist until a matching role-switch rule. Other workflows may deliberately assign a template on every state entry.

### Customize a language lesson

Save this object at `custom_data.language_learning` (do not replace unrelated `custom_data`):

```json
{
  "target_language": "Japanese",
  "explanation_language": "German",
  "level": "A1",
  "lesson_goal": "Greetings and introductions",
  "reply_style": "Two short sentences and one useful practice question"
}
```

Without overrides, the tutor supports French/Japanese practice with German explanations. The learner's explicit request wins over defaults. Text transcripts are not proof of pronunciation; the template does not claim that audio played or promise medical benefits.

## Semantic blueprints

Each primary persona imports a JSON blueprint from `templates/blueprints/` and renders it through `templates/shared/blueprint.poml`. The contract defines **what happens to whom**, not just a vague role label:

```json
{
  "scene_goal": "Increase tension by showing defiance",
  "participants": [
    {"name": "Onyx", "role": "Agent", "description": "black cat"},
    {"name": "Red Ball", "role": "Patient", "description": "mysterious"},
    {"name": "Grandfather Clock", "role": "Source_of_Threat", "description": "ancient, looming"}
  ],
  "action_to_complete": {"predicate": "play with", "agent": "Onyx", "patient": "Red Ball"},
  "constraints": ["Preserve the stated participants and action"],
  "success_criteria": ["Defiance increases the scene's tension"],
  "output_format": "One suspenseful sentence"
}
```

Save overrides at `custom_data.semantic_blueprint`, or use `cl_data.semantic_blueprint` for a workflow-provided contract. The custom-data object takes precedence over the workflow object; the selected override merges with the persona defaults. `action_to_complete` merges by field; other arrays/fields replace their defaults. The blueprint is task data, not authority to bypass permissions. It is not printed to the user unless requested.

Templates ask for private planning/checking and concise public plans, assumptions, evidence and results—not exposed chain-of-thought. Simple questions still receive direct answers. This is a structured prompt/task contract, not a modification of the model's underlying inference algorithm or a guarantee of correctness.

`shared/runtime.poml` advertises the current input, selected workflow/template/skill, registered skills and required parameters, enabled tools, and **this user's** facts, preferences, topics and memory variables. Only successful memory tool results justify saying information was saved. Tool descriptions and old memories are not new authorization.

## Actual SM grammar

```ini
@name "Example coding phases"
@version "1.0"
@steps [plan, implement, verify, done]

[state plan]
settings.system_template = "tasks/plan"

[state implement]
settings.system_template = "tasks/code"

[state verify]
settings.system_template = "tasks/test"

[state done]
settings.system_template = "tasks/done"

[overrides]
if custom_data.user_prompt =~ "(?i)^reset role$" -> settings.system_template = "standard"
```

`@steps` supplies the ordered states for `agent_next`. State assignments currently store string-compatible values; use normal typed context tools for JSON booleans/objects. `[auto]` accepts ordered `condition -> use STATE` rules (first match wins). `[transitions]` uses `FROM -> TO : when CONDITION`; `[overrides]` uses `if CONDITION -> dotted.key = value`. The old inline `transition -> ... on next` / `auto_rule:` snippets are not the supported parser grammar.

Conditions support dotted paths, comparisons, `=~` / `!~` regexes and quote-aware `&&` / `||`. Tool history lives at `custom_data.used_tools` and `custom_data.tool_history`, not an automatically populated root `used_tools`. Regex backslashes are literal in `.sm` files: use `\b` or `\d`, not JSON's doubled-backslash encoding. Do not use turn counts as proof a task finished. The shipped `coding.sm` and `self_learning.sm` use explicit phases instead.

## Repair an incomplete onboarding installation

Older onboarding code installed only the previous template list and created the obsolete `contextlanguage/` directory; it omitted the new includes, `contexts/standard.sm`, native skills and bitmap icons. This caused the first paired message to fail with `Workflow routing failed: CL IO Error: No such file or directory` and the logo to return 404. Changing `sm_file` does not create the missing file.

Use the **updated binary**, not interactive onboarding, to repair the working directory used by `praxis run`:

```bash
/path/to/updated/praxis repair-assets --directory /path/to/installation --update-dashboard
```

- Adds all missing bundled workflows, templates/imports, skills and logo/favicon assets.
- Preserves existing templates, workflows, skills, `.env`, secrets and databases; does not re-pair users or change activation.
- `--update-dashboard` explicitly replaces differing bundled static UI files and saves their previous bytes under `.praxis-asset-backups/<unique-id>/`. Omit the flag to preserve every existing file.
- Refuses symlinked asset destinations. No providers, databases or services are opened/started; even `.env` is not loaded by this command.
- Refresh/hard-refresh the browser after updating JS/CSS. A running binary with the new routing/asset endpoints reads these repaired files on subsequent requests; installing a newer binary itself still requires your normal service update.

Fresh `onboard --interactive` now uses this same complete asset bundle. **Do not rerun onboarding just to repair files**: onboarding is a configuration wizard and can rewrite settings/credentials. Node and a working `POML_CLI` remain separately required; asset repair does not install the renderer.

## Validation and deployment

From the source root:

```bash
export POML_CLI=/absolute/path/to/poml/js/cli.js
python3 scripts/test_poml_templates.py
python3 scripts/test_language_learning_poml.py
python3 scripts/test_skills.py
python3 scripts/check_context_docs.py
node scripts/test_ui_static.js
CARGO_BIN=cargo bash scripts/test_workflow_backend.sh
# Requires Playwright + its Chromium, installed separately:
PLAYWRIGHT_BROWSERS_PATH=/tmp/praxis-ui-browser-cache node scripts/test_ui_browser.js
```

[`examples/poml-test-context.json`](../examples/poml-test-context.json) contains synthetic identity, current input, settings, workflow state, skills/tools, active-skill metadata, memory, token/history metadata, tutor settings, semantic roles and optional legacy task inputs. The fixture-coverage check also requires every typed `Context` and `ContextSettings` field. The all-template check renders **every `.poml` file recursively**, from a different cwd, against full, sparse, null and chat cases. It verifies sentinel content as well as strict exit status. No live context dump or database is needed.

Strict rendering requires Node and the actual Microsoft JavaScript CLI in the service environment. It does not fall back to brace substitution. Templates are trusted local code, not a sandbox; imports need authorization. See [Skills](SKILLS.md) for safe template saves and targeted Rust test commands.

Old absolute workflow paths and the former `contextlanguage/` / `data/contexts/` search roots must be relocated/reselected under the canonical `contexts/` directory. Explicit session helpers now read/write one scoped row without double-appending a session ID or silently changing activation. Durable memory remains shared across one user's sessions, not across different user IDs.

Changes are **source-only** until rebuilt and deployed. Ship the updated binary and complete `templates/`, `contexts/`, `skills/` and `static/` directories, including blueprint/include dependencies. The cactus/dune `logo.png`, favicon and touch icon all derive from `static/logo-source.png`; `scripts/build_logo.mjs` reproduces the sizes using `sharp`. Normal bot startup registers `/skill`. Do not restart an active service or change user selections merely to run tests.
