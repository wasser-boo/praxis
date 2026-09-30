# Workflow, POML, memory and interface change report

All changes are in the **source checkout**. No live database, service configuration, deployed binary or active user selection was changed. No commit, deployment or service restart was performed. Existing unrelated setup/voice/provider changes were preserved.

Herdr coordination used verified session `default`, workspace `w7`, coordinator `w7:p5`: `w7:p2` implemented backend changes and `w7:p4` implemented/reviewed UI changes; both reported done. The coordinator independently reviewed screenshots/code, fixed remaining TUI cursor issues and reran the final tests. Earlier timeouts/failing intermediate test runs are superseded by the successful final results below.

## Implemented

- **Persistent Discord skills:** `/skill skillname:NAME` selects instructions for subsequent tasks, `off` clears them, and omitted name/`list` lists available skills. Current task text supplies required `code`, `error` or `user_request`. Pairing, guild/channel checks and disabled tools remain enforced. Selection never runs scripts. Active instructions are appended even when a custom system template omits them.
- **POML authoring skill:** native `skills/poml_templates/` includes Microsoft syntax guidance, runnable examples and a real-CLI validator. `use_skill` is registered and wired through both dispatchers and the public LLM tool loop. `update_template` validates prospective content before replacement, rejects unsafe destination paths and detects direct/transitive include cycles without overwriting the old file.
- **Canonical workflow wiring:** `sm_file` / `settings.sm_file` replace `cl_file`, with legacy-input normalization; `sm_data` is canonical, with legacy `cl_data` input compatibility. Workflow routing runs before user/system rendering on both message paths, and agent iterations refresh context after tools. Dashboard and runtime use the same `contexts/` root. Missing/malformed selections fail visibly.
- **Semantic profiles:** `standard.poml`, `language_instructor.poml`, `code_assistant.poml`, `researcher.poml` share runtime context and JSON blueprints defining goals, participants, predicate/agent/patient, constraints, success criteria and output format. Overrides are customizable through `custom_data.semantic_blueprint` / `sm_data.semantic_blueprint`. Planning/checking stays private; useful plans and evidence are public.
- **Editable persona switching:** `contexts/standard.sm` routes explicit requests such as “Be a language instructor”, keeps the persona for ordinary follow-ups, preserves manual template choices and supports resetting to standard. The five shipped workflows now use actual parser syntax. All original template names remain compatible; previous prompt/workflow contents are archived outside template discovery.
- **Current context and capabilities:** shared POML context includes current raw `user_prompt` / `user_message`, settings, workflow state, registered skills, enabled tools and user-scoped memory. Preview does not use the latest assistant/tool message as user input. Chat status displays the actual system template and active skill, separately from the workflow template stack.
- **Memory correctness:** transactional/idempotent snapshots, atomic read-modify-write updates, typed JSON preferences/variables, real inserted IDs, deduplication, latest-value precedence, no 100-row truncation, scoped deletion, visible storage errors and explicit clearing. Legacy preferences migrate once and do not reappear after clearing. Memory remains shared across one user's sessions. Explicit session helpers no longer double-append IDs or silently change activation.
- **Web interface:** mobile dashboard sizing, wrapping long tool descriptions, accessible working toggles, light-theme chat selectors, conversation/menu drawers, independently scrolling messages and an always-visible mobile composer/send button.
- **TUI:** terminal-column-aware Unicode wrapping/alignment, consistent bubble borders, long-label handling, automatic narrow-sidebar collapse and composer space reserved before transcript space. Empty/long/multiline input and cursor movement remain visible, including very small terminals; attachment rows cannot hide the cursor.
- **Matching branding:** one generated cactus-and-desert-dunes bitmap supplies the app logo, PNG favicon, multi-size ICO and touch icon. `scripts/build_logo.mjs` reproduces all sizes from `static/logo-source.png`.

## Verified

| Check | Result |
|---|---|
| Scoped Rust suite, including real POML integration tests | **188 passed** across 19 non-overlapping groups |
| Every shipped `.poml`, recursively, with full/sparse/null/chat contexts | **84 strict renders passed** across 21 templates |
| Additional tutor renders | **9 passed** |
| Skill CLI/helper/stubbed-tmux checks | **39 passed** |
| Fixture/reference coverage | **11 root + 82 settings = 93 typed fields**, plus runtime/task variables |
| Chromium synthetic-API browser checks | **6 viewport/theme cases passed**: 360×640 and 390×820 dark/light, 768×900 and 1280×800 dark |
| Browser assertions | No page/chat horizontal overflow; long-description/toggle bounds and toggling; actual template/skill labels; composer visibility/clickability; independent message scrolling; mobile menu/drawer open/close |
| Branding reproduction | All four derived assets reproduced **byte-for-byte** |
| Syntax/whitespace | Python, JavaScript, shell, JSON and `git diff --check` passed |

Rust tests include actual repository SM routing/stickiness/reset and template-resolution checks, memory rollback/concurrency/clearing/isolation, session roundtrips, Discord authorization/selection and TUI TestBackend assertions. The full unrelated Rust suite was not run. Browser tests use local synthetic endpoints, not a running Praxis service. No live LLM/voice provider was called during testing; image generation was a separate requested asset-creation step. Actual tmux operations remain untested because tmux is not installed.

Reproduce with `scripts/test_workflow_backend.sh`, `scripts/test_poml_templates.py`, `scripts/test_skills.py`, `scripts/check_context_docs.py`, and `scripts/test_ui_browser.js`. Full synthetic inputs: `examples/poml-test-context.json`. Current logs/screenshots: `/tmp/praxis-poml-audit/` (temporary development artifacts).

## Follow-up: onboarding installation and Chat navigation regressions

The initial checks above missed the standalone onboarding asset list and actual Chat → Overview link clicks. The subsequent user report exposed both gaps; the installation directory itself was not repaired during investigation/testing.

- **Root cause of first-message/branding failures:** interactive onboarding created the obsolete `contextlanguage/` directory and only the previous template/static subset. It omitted `contexts/standard.sm`, required POML imports, skills and PNG/ICO assets. Embedded dashboard HTML could therefore reference images which were never installed.
- **Packaging fix:** `src/assets.rs` provides one explicit 58-file bundle, shared by onboarding and the new offline `repair-assets` CLI. Missing files are installed atomically per file. Existing prompts/workflows/skills remain untouched; an explicit `--update-dashboard` backs up differing static files before replacement. Configuration/secrets/databases are neither loaded nor changed by repair. Symlink destinations are rejected. Coverage tests guard against another incomplete shipped-template/skill/workflow list.
- **Navigation root cause/fix:** a reproduced Playwright click failure proved the floating Chats button intercepted Overview on mobile and the dashboard-menu toggle on desktop. The Chats toggle is now mobile-only and hidden while the main navigation is open; leaving Chat restores the normal content layout and menu state. Menu controls report accessible expanded states.
- **Diagnostics:** missing workflow roots/selections now identify the directory and selection and provide an asset-repair hint, without silently changing the chosen workflow.

Follow-up verification:

| Check | Result |
|---|---|
| Expanded scoped Rust suite | **196 passed** across 20 non-overlapping library groups, including fresh-install first-message/active-skill strict rendering |
| Repair CLI parsing and binary build | **1 additional CLI test passed**; updated binary built at `target/release/praxis` |
| Actual repair CLI in a synthetic installation | **58 assets created**; second run preserves all 58; explicit dashboard update backed up old bytes; synthetic config/DB/secret sentinels unchanged |
| Strict rendering from the CLI-installed template tree | **84 passed**, 21 templates × full/sparse/null/chat contexts |
| Installed-assets Chromium checks | **12 viewport/theme cases passed**: six with installed HTML, six with previous onboarded HTML; actual Chat → Overview → Tools → Chat clicks, HTTP image/icon responses, decoded login/message images, composer and scrolling assertions |
| Context docs, JS/Python/shell syntax and whitespace | Passed |

Evidence: `/tmp/praxis-onboarding-fix/{rust-final.log,build.log,poml-installed-v2.log,browser-installed-v2.log}` and adjacent exit files; screenshots in `ui-installed/` and `ui-legacy-html/`. `browser-before.log` records the original intercepted click. The renderer's initial concurrent 180-second harness timeout was followed by a complete successful isolated run. All execution used synthetic databases/endpoints and local assets; no live provider, configuration, deployment or service restart was involved. The existing installation's public HTML was only read to verify compatibility.

See [safe installation repair](POML_WORKFLOWS.md#repair-an-incomplete-onboarding-installation). `scripts/test_ui_browser.js` accepts `PRAXIS_TEST_STATIC_DIR` and optional `PRAXIS_TEST_INDEX_FILE`; `scripts/test_poml_templates.py --templates-dir /path/to/installed/templates` verifies an installed tree.

## Deployment requirements

Rebuild and deploy the normal Praxis binary with its required features, plus complete `templates/`, `contexts/`, `skills/` and `static/` directories. Configure Node and `POML_CLI` in the **service** environment; strict runtime rendering does not use fallback substitution. Normal bot startup registers `/skill`.

Old absolute workflow paths or former `contextlanguage/` / `data/contexts/` selections need relocation/reselection under `contexts/`. Use canonical `sm_data`; legacy `cl_data` values migrate on normal context loading/saving. Templates are trusted local code, not a sandbox, and imports require authorization. No production restart or activation is included in this change.

Usage and customization: [POML workflows](POML_WORKFLOWS.md), [Skills](SKILLS.md), [complete context reference](CONTEXT_VARIABLES.md).
