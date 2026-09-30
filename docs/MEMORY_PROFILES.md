# User-private memory profiles

Memory is no longer one bucket for every persona. Profile **contents belong to
one Praxis user ID**; the selection belongs to `(user ID, session, persona)`.
A fork/new user ID does not inherit another user's profiles or shared facts.
Profiles organize memory, not file/shell permissions or a security sandbox.

## Defaults and preservation

- `standard` (including the `system` alias) uses the existing memory table and
  legacy preference importer. Old facts, variables, SRS and XP remain there.
- Other personas default to their template name, e.g. `language_instructor`,
  `code_assistant`, `researcher`. `language_learning` is a tutor alias.
  Path-shaped state templates also keep that name: `states/standard/standard`
  is a separate profile, not an alias for legacy `standard` memory.
- The tutor's daily-quiz aliases use the same `language_instructor` category.
- Missing categories expose empty memory with `profile_exists=false`, **never
  another persona's facts**. The LLM should create and then load the category.
- An explicitly selected relevant custom profile is remembered for that
  session/persona. Switching back restores that selection. The same profile can
  be explicitly used by related tasks; its content is shared across that user's
  sessions, while selections are not.
- Nothing bulk-copies or reclassifies old memory. Move selected old learning data
  only after review/authorization; do not copy the entire legacy bucket.

Schema migration 12 adds two tables without rewriting existing memory rows.
Back up the database before deploying a binary that runs migrations. Existing
customized POML/workflows are not overwritten automatically. Native prompt
rendering also appends the scoping contract for custom templates.

## LLM tools

Discover schemas using `search_tools` first (new schemas become callable on the
next model turn). These are not additional always-loaded core tools. Existing
tool enablement rules remain enforced; unavailable tools do not imply persistence.

```json
{"name":"language_instructor"}
```

Pass this to `memory_profile_create` if missing, then `memory_profile_load`.
Creation is idempotent and never clears an existing profile; it does not select
it. Loading requires an existing profile and never changes another session or
persona's selection. `memory_profile_list` reports available names; `memory_get`
and the prompt expose the current profile/existence state. `shared` cannot
become the active profile.

Then read before writing:

```json
{"key":"xp"}
```

`memory_get` returns `profile`, `key`, `exists` and `value`. For example, after
reading `value: 10` from `profile: "language_instructor"`:

```json
{"key":"xp","value":12,"expected_profile":"language_instructor","expected_value":10}
```

Pass this to `memory_set`. Wrong profiles or changed values fail without a write;
re-read and reconcile rather than blindly retrying. Nonstandard writes require
`expected_profile`. Omission remains compatible only with `standard`. Null
removes a variable; null `expected_value` means the key was absent. Other keys
are preserved. A value is limited to 64 KiB; a named profile to 8 MiB; at most
128 normal profiles including the virtual standard profile may exist per user.
Names are bounded ASCII identifiers; nested names such as
`language_instructor/french` are logical names, **not filesystem paths**.

`learn_fact`, `learn_preference`, `learn_topic` and parsed learning tags also use
the current profile. They do not silently fall back or create a missing category.
The POML `memory` projection includes only its facts/preferences/topics/variables,
plus `profile`, `profile_exists`, `profile_loaded`, `profiles` and separate `shared`.
`learning_profile`, `srs_items` and `xp` remain ordinary typed variables inside a
profile, not new context settings. Do not use `set_context` to write memory.

## Shared: exceptional, not a second dumping ground

Shared is **private to the same user**, not shared between people/chats with
different IDs. It currently permits only `name`, `pronouns`, `time_zone`, each a
nonempty string of at most 256 bytes. No lesson history, SRS, XP, projects,
arbitrary facts/topics/preferences, or sensitive inferred details belong here.
Use it rarely, only when the user authorized remembering genuinely general data.

Read using `memory_get({"scope":"shared","key":"name"})`, then:

```json
{"scope":"shared","expected_profile":"shared","key":"name","value":"Alex",
 "expected_value":null,"reason":"The user explicitly asked to remember their name across personas."}
```

A short explicit reason is mandatory for shared writes. It is a guardrail, not
proof of consent: the LLM must still have real authorization. Nothing
promotes facts into shared automatically. Shared cannot be loaded as a persona.

## Dashboard/API

Memory displays the active category, a read-only profile-view selector, its
custom variables and a separate Shared section. **Browsing does not load** a
profile for the agent. Stale responses must not appear under another user.

Authenticated routes:

- `GET /api/memory/{user_id}`: current-profile view and metadata.
- `GET /api/memory/{user_id}?profile=language_instructor`: inspect a named profile
  without changing selection; missing profiles have `profile_exists=false`.
- Existing `PUT /api/memory/{user_id}` accepts optional `profile`, plus its
  existing `custom_variables`, `user_preferences`, `learned_facts`, `last_topics`
  sections. Supplied sections replace those sections; omitted sections remain.
  Unspecified profile uses the current category. Shared permits only the three
  allowed variables and requires `reason`; fact/topic/preference writes there
  are rejected. Missing named profiles return 404, not implicit creation.

These remain dashboard/operator APIs, not LLM tool permission bypass instructions.
Deleting a session removes its selections, not durable profile contents. Deleting
one user removes that user's profiles/selections and preserves other owners.

## Offline checks

```sh
cargo test --locked --offline --lib memory_
cargo test --locked --offline --lib tools::memory
node scripts/test_memory_ui.js
# With a locally installed Microsoft POML CLI:
POML_CLI=/path/to/cli.js python3 scripts/test_prompt_discovery.py
POML_CLI=/path/to/cli.js PRAXIS_REQUIRE_POML=1 cargo test --lib -j 1 \
  tool_chain_discovery_loads_memory_only_for_current_task_on_both_paths -- --ignored
```

Tests use temporary databases/synthetic data. They do not migrate a live database,
change profiles in a running installation, or call an LLM provider.
