# Progressive tool discovery and learning memory

## Small initial capability set

Both chat/tool-loop and multi-turn agent paths send only **12 core schemas**:
`search_tools`, `search_skills`, `use_skill`, `get_context`, `set_context`,
`read_file`, `write_file`, `edit_file`, `execute_terminal`, `agent_complete`,
`agent_feedback`, `agent_next`. Disabled core tools are omitted too.

`search_tools({"query":"brave web search", "limit":1})` searches enabled builtin
and plugin names/descriptions. Its matching schemas become callable on the
**next model turn in this task**, not in the same tool-call batch. Searches
return at most 8 results (default 5). At most 24 additional schemas can be loaded;
`replace:true` replaces previous discoveries while keeping the core. Refine
queries rather than dump the catalog. Selection is never written to context or
tools.json; completion/cancellation clears it and other users/tasks do not share
it. Disabled tools remain disabled and are rechecked before execution.

`search_skills` / `use_skill` continue to load bounded metadata/instructions
lazily, respecting hidden/user-only flags. Loading a skill is not executing it
and does not automatically load all referenced tools. The shared POML runtime
explains discovery to all standard personas/role/task aliases instead of
repeating every tool description. Provider tool schemas and runtime `tools`
use the same selection. Custom POML cannot broaden callable permissions.

The core is not a security sandbox: existing file/shell permissions remain.
Finding a paid or consequential tool does not authorize using it.

## Durable memory versus context custom_data

- `custom_data.language_learning`: per-context lesson configuration (language,
  level, goal, reply style). Set using normal context controls.
- `memory.variables`: read-only POML projection of durable Memory custom variables.
  Dashboard **Memory → Custom Data** displays this same data in the selected
  [memory profile](MEMORY_PROFILES.md), not one global bucket per persona.
- Discover `memory_profile_create`, `memory_profile_load`, `memory_profile_list`.
  Load the relevant category (e.g. `language_instructor`); create it if missing.
  Existing `standard` data is retained, never bulk-copied into other categories.
- `memory_get({"key":"srs_items"})`: read a JSON value, existence and profile.
- `memory_set({"key":"srs_items", "value":[...], "expected_profile":"language_instructor", "expected_value":[...]})`:
  atomically replace just this user's current-profile variable. JSON types are preserved; null
  deletes that key. `expected_value` is the exact previously read value, null for
  an absent key. Conflicts fail and require a re-read; identical retries succeed.
  Individual values are limited to 64 KiB. Other keys/users remain unchanged.

Search `memory` to load these tools first. **Do not use set_context with
memory.variables.*:** memory is not a saved Context namespace. Existing
learn_fact/preference/topic tools remain available through discovery and also
use the current profile. Separate shared memory is user-private and exceptional:
only authorized name/pronouns/time_zone, explicit scope and reason; no learning
state or automatic promotions. See [MEMORY_PROFILES.md](MEMORY_PROFILES.md).

## Language instructor and daily quiz

The instructor and compatibility alias `language_learning`, plus `daily_quiz`,
use `learning_profile`, `srs_items` and numeric `xp` from the durable
`language_instructor` profile (or a relevant explicitly selected tutor profile). Context
configuration wins over the saved profile; the current learner request wins over
both. The profile can record consent to tracked practice. Do not infer consent
from merely mentioning a language or silently save sensitive details.

Example SRS item:

```json
{"id":"fr-bonjour", "language":"French", "item":"bonjour", "translation":"hello",
 "due":"2026-09-15T12:00:00Z", "interval_days":1, "lapses":0}
```

Runtime provides `utc_now` as an ISO UTC timestamp. The prompt shows at most five
valid due cards in the configured language, oldest first. Future/other-language
cards remain stored, not mislabeled as due or deleted. The generic runtime shows
only variable names, not the whole deck. Use memory_get before updating the full
deck so a five-card prompt snapshot never replaces the complete collection.

The POML instructions define the review policy: wait for an actual answer;
correct unaided recall doubles the interval (max 60 days) and earns 2 XP; wrong
recall resets to 1 day and increments lapses. Ambiguous transcripts are ungraded.
Record stable review IDs to avoid repeated grading; never fabricate scores.
SRS scheduling/grading is agent-directed, not an autonomous scheduler. Card and
XP writes are separate atomic operations; reconcile partial failures explicitly.
Saved progress survives messages/restarts, but no scheduled reminders are created
without a separate explicit request. Memory is scoped by internal user ID; a new
forked dashboard chat has a different ID, not automatically shared human memory.

## Voice independence

`settings.web_chat_tts=false` mutes automatic dashboard speech only. Discord
normal reply TTS still follows `settings.use_tts`; voice inputs also permit voice
replies. Neither discovery nor learning-memory work changes these controls.

## Validation

Use synthetic contexts only, not private production context dumps:

```bash
export POML_CLI=/path/to/Microsoft/poml/js/cli.js
python3 scripts/test_poml_templates.py
python3 scripts/test_prompt_discovery.py
python3 scripts/test_language_learning_poml.py
python3 scripts/test_brave_search.py
node scripts/test_memory_ui.js
cargo test --lib tools::discovery
cargo test --lib tools::memory
cargo test --lib tool_chain_discovery -- --ignored
```

Validate the deployed template tree with `--templates-dir /path/to/templates`.
No provider calls are made by these checks. Builds/deployments retain Songbird.
