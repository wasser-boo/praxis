# Bug audit — tools, tool groups, state machine, templates

Result of the review that accompanied the `activated_tools` / `[tool_groups]` /
SM-routing work. Status: **fixed** = changed in this pass (covered by tests
unless noted); **open** = confirmed but not changed; **watch** = design
caveat, not a defect on its own.

## Fixed

1. **Plugin tools leaked past the state allow-list** (`src/tools/registry.rs`).
   In `DescriptionOnly` mode every enabled plugin tool (e.g. `brave_web_search`)
   was appended with an empty schema even when the state listed tools
   explicitly. Plugins now follow the same allow-list as built-ins.
2. **All built-in tools dropped when a state used `tool_groups` without
   `full_tool_categories`** (`registry.rs`). `allowed_cats` became `Some([])`,
   which filtered out every built-in — states got only the leaked plugin tools.
3. **`[tool_groups]` section unreachable in `.sm` files** (`src/sm.rs`). The
   parser had a branch for it but rejected the header as "Unknown section", and
   `apply_tool_groups` *activated* every group instead of defining them; the
   recursive resolver had no cycle guard. Now: definitions merge into
   `settings.tool_group_definitions`; states opt in via `settings.activated_tools`.
4. **Hardcoded tool groups removed**. No built-in groups; the shipped
   `contexts/standard.sm` defines its own. `activated_tools` mixes group names
   and single tools (`["coding", "execute_terminal", "brave_web_search"]`).
   `tool_groups` / `full_tool_schemas` remain as legacy aliases.
5. **Model-driven state switching never happened** (`contexts/standard.sm`).
   Roles were wired with `[transitions]`, which only `agent_next` evaluates.
   `set_context("sm_data.role", …)` therefore changed nothing. Replaced by
   `[auto]` rules, evaluated on every routing pass.
6. **Routing froze after the first state switch**
   (`src/gateway/workflow_actions.rs`). `plan()` skipped routing whenever
   `system_template` was not `standard`, so the SM's own
   `states/code/code` selection disabled all further routing. Now only a
   template that no state of the active workflow produces counts as a manual
   choice.
7. **`transition_to` stored state values as raw strings** (`sm.rs`). An array
   like `settings.activated_tools = [...]` became `"[\"…\"]"`, and the
   context failed to deserialize on `agent_next`. Uses the same JSON typing as
   `apply_to_context`.
8. **Every `templates/states/*/…poml` failed to render.** POML resolves nested
   includes relative to the *including file*; `shared/state_base.poml` used
   `../../shared/…`, which pointed outside `templates/`. Only visible with a
   real POML CLI, so no test caught it. (Verified with poml 0.0.8 and the
   `../poml` source; no automated test — needs `POML_CLI`.)
9. **`states/teach/teach.poml` had a raw `&&` in text content** — XML parse
   error, template never rendered. Moved into a `<let>`.
10. **Per-state blueprints ignored.** State templates set `blueprint`, but
    `shared/blueprint.poml` only read `persona_blueprint`; every state got
    `blueprints/standard.json`. Both are honored now.
11. **Review persona loaded the language-instructor blueprint**
    (`templates/personas/review.poml`). Points at `states/review/review.json`.
12. **State templates had no way back.** Once in `code` etc. the system prompt
    said nothing about `set_context("sm_data.role", …)`; added
    `shared/roles.poml` and a `role_switch` group (`set_context`, `get_context`)
    activated in every state.
13. **`search_tools` discoveries were never callable.**
    `task_control::selected_tools` fed only the POML `tools` variable, not the
    request's tool list, so a discovered tool was rejected as "Unknown tool".
    `build_tool_definitions_for_user` now merges the task-local selection into
    the state's allow-list.
14. **Duplicate registry entries** — `understand_image` and `update_template`
    were registered twice, producing duplicate function names in requests.
15. **Assets resolved against the shell's cwd.** ~15 call sites use
    `Path::new("templates")`, `"contexts"`, `"./plugins"`. `pin_install_root()`
    (`src/main.rs`, `config::resolve_install_root`) now chdirs to `ROOT_DIR`,
    else the cwd if it holds `templates/`, else the executable's directory — so
    `~/praxis/praxis` uses `~/praxis/templates` wherever it is started from.
16. **System prompt too large for small contexts.** `states/standard/standard`
    rendered ≈970 tokens, the root `standard.poml` ≈2300 (full blueprint JSON,
    duplicated user prompt, duplicated tool list, 7 KB memory boilerplate).
    Now ≈460 tokens (root fallback ≈460–620 depending on persona). Other state
    templates: 620–760; `teach` ≈1700, `20-tasks` ≈2000 (not reduced).

17. **`tool_discovery_mode = "None"` now sends only activated tools**
    (`filter_tools_for_request`); previously every allowed built-in was sent
    with an empty schema.
18. **`20-tasks.sm` offered ~45 tools with empty schemas.** It now defines
    `[tool_groups]` and per-state `activated_tools`.
19. **Sticky state variables.** `apply_to_context`/`transition_to` now reset
    every variable another state declares but the resolved state does not
    (falls back to a `_default` value, else removed). Keys are compared
    canonically (`cl_data.*` ≡ `sm_data.*`).
20. **Decision router vs `[auto]` rules.** While a decision profile is active
    (`decision_profile` set, `use_decision_router` true) auto rules are
    skipped, so the router's chosen state is not overridden during routing.
21. **`_default` no longer selectable** as `active_state` (SM fallback and
    `route_context_once` validation).
23. **Gated render test added** (`src/gateway/template_render_tests.rs`): with
    `POML_CLI` set it renders all 25 shipped templates through the real CLI,
    checks every SM state's template rendered, and caps the standard prompt at
    2800 chars. `PRAXIS_REQUIRE_POML=1` turns a missing CLI into a failure
    (use in CI/Docker where the CLI exists). Without `POML_CLI` it prints a
    skip notice.
    The first run found four more dead includes left over from the
    "unified routing" refactor: `tasks/code`, `tasks/test`
    (`../code_assistant.poml`), `tasks/review` (`code_review.poml`),
    `tasks/daily_quiz`/`language_learning` (`language_instructor.poml`) — now
    point at `personas/*.poml`.
26. **Provider changes required a restart.** `GatewayState.llm` is now a
    hot-swappable `LlmHandle`; `/login`, `/logout` and dashboard secret updates
    rebuild the router in place.

## New in this pass: `/login`

- TUI `/login` / `/logout` (`src/tui/app.rs`), gateway
  `GET /v1/providers`, `POST /v1/providers/login` (`src/gateway/client_api.rs`,
  `src/gateway/providers.rs`). Runs on the gateway host, so a remote TUI logs
  the remote machine in. Prints `/context set settings.provider=… settings.model=…`
  hints on success.
- Codex provider (`src/gateway/llm/codex.rs`): ChatGPT-subscription OAuth
  tokens (imported from the Codex CLI's `~/.codex/auth.json`, or via
  `codex login --device-auth` spawned on the gateway host, or pasted with
  `--auth-json`) against the Codex Responses API, with streaming, tool calls
  and automatic token refresh. Tokens are stored under `custom.codex_auth`.
- The gateway now retains the master key in process memory
  (`secrets::retain_master_password`) so logins and refreshed tokens persist;
  `PRAXIS_RETAIN_MASTER_KEY=0` opts out.

## Open

22. **`sm_data.persona_roles` lives in `_default`**, applied only when
    missing — a context that already has a different value keeps it.
24. **`teach`, `20-tasks`, `tasks/transcript_check`, `daily_quiz` templates are
    still 1.6–2.3 k tokens** — too large for a 4096 context with tools and
    history.
25. **`tool_result_limit`, `download`, `feedback_*` settings are stored but
    unused** (noted in `docs/CONTEXT_VARIABLES.md`); consider removing.
27. **Codex provider is untested against the live service.** Request/response
    shapes follow the Codex CLI (Responses API, `chatgpt-account-id`,
    `OpenAI-Beta: responses=experimental`); wiremock covers refresh + SSE
    parsing only. Rate-limit headers and `response.incomplete` are not
    handled specially.
28. **`codex login --device-auth` output parsing is heuristic** (URL + a line
    containing "code", bounded to 20 s / 12 lines). If the CLI changes its
    wording the TUI still shows the raw lines, and the background import still
    completes when the CLI exits 0.
29. **Endpoint probe for `llamacpp` accepts 404 on `/models`** (older servers
    lack it) — a wrong URL that answers 404 passes the check.
30. **Master-key retention widens exposure** from "startup only" to "process
    lifetime". Acceptable for the threat model documented in `main.rs` (agent
    shell cannot read process memory), but it is a policy change; see
    `PRAXIS_RETAIN_MASTER_KEY`.
31. **Dashboard `/secrets` API cannot set `codex_auth` meaningfully** (custom
    values are masked/free-form); use `/login codex` instead.

## Watch

- `activated_tools` with unknown names is silent (kept as tool names that match
  nothing). Intentional so plugin names work before the plugin is installed;
  a warning at routing time would help authors.
- Legacy `tool_groups`/`full_tool_schemas` are still merged; remove after
  users' contexts have migrated.
- Root `templates/standard.poml` and `states/standard/standard.poml` now
  overlap; the root file is only the fallback for stale/unknown templates.
- Render test budget (2800 chars for the standard prompt) is a tripwire, not a
  guarantee of fitting 4096 tokens with large tool schemas.
