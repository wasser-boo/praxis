# Chat history refresh investigation — 2026-09-29

Baseline: `testing` at `ad9d08c7f8a826f67fe3e4387ead2311fdf90761`.
Tracking: [upstream #1](https://forgejo.the.grid/Marvin/praxis/issues/1),
[fork #1](https://forgejo.the.grid/Praxis/praxis/issues/1).

## Scope

Investigate lag increasing with chat history length. Changes are made in an
isolated fork checkout, not the running installation. Synthetic fixtures only:
no user history, tokens, live provider calls or backend data changes in tests.

This investigation confirms a web main-thread rendering bottleneck. It does
**not** establish a memory leak, compromise, or the sole cause of VM-wide lag.
TUI layout performance and feature requests are separate work.

## Investigation log: hypothesis → test → evidence → conclusion

1. **Full DOM lookup per saved row causes quadratic work.** Execute the unchanged
   `chatSavedMessage`, `chatBindSavedMessage`, `chatOrderSavedMessage` and
   `renderChatMessage` functions in headless Chromium with actual DOM nodes.
   Seed alternating saved user/assistant rows, replay unchanged content, and
   instrument selectors; unrelated TTS/timer effects are stubbed.

   | Rows | Selector calls | Returned node references | Sample render time |
   | ---: | ---: | ---: | ---: |
   | 100 | 150 | 12,500 | 5.9 ms |
   | 500 | 750 | 312,500 | 123.6 ms |
   | 1,000 | 1,500 | 1,250,000 | 500.8 ms |
   | 2,000 | 3,000 | 5,000,000 | 1,821.4 ms |

   **Confirmed:** `chatSavedMessage` materializes all rows per input message;
   assistant rendering also materializes all assistant rows even after an ID
   match. Doubling rows quadruples returned references. Timings are diagnostic,
   environment-dependent samples, not acceptance thresholds.

2. **A refresh-local ID map removes the first scan.** Add a browser regression
   using the complete dashboard/HTTP history pipeline, before implementation.
   Baseline fails at 100 unchanged user rows: 100 selectors, 10,000 references.
   Add the per-refresh map; user cases become 100/500/1,000 references, but the
   mixed case still fails (100 rows, 51 selectors, 2,600 references).
   **Confirmed:** saved-ID lookup is fixed; assistant lookup remains independent.

3. **Scan provisional assistants only when no saved ID matched.** Move the
   assistant query behind the existing missing-row guard and rerun all history
   cases. **Confirmed:** both user-only and mixed histories pass, with one
   saved-row query per refresh rather than one per message.

4. **Command cards repeat the same pattern.** Extend the browser regression to
   100 unchanged saved terminal commands before changing that path. It fails
   with 10,000 returned references. Index terminal cards by call ID once per
   refresh, preserving the existing message-ID/provisional-row matching rules.
   **Confirmed:** 100 returned references after the change. Final normal/mixed
   replay performs two queries (saved rows plus terminal cards), returning N
   references for N ordinary saved messages.

## Fix

`refreshChatMessages` builds saved-message and terminal-call indexes after
pruning, for its synchronous render batch only. New rows update the indexes
within the same batch, preventing duplicate inserts. The indexes are discarded
before the next refresh; no persistent cache holds detached/session-specific
DOM nodes. Direct SSE calls retain their existing lookup behavior. Assistant
provisional matching is skipped for already-resolved saved identities.

Content, agent context, stored history, tool output and copy behavior are not
truncated or altered.

## Verification and prevention

New `history-refresh-linear` case in `scripts/test_chat_history.js`:

- Runs the real HTTP refresh and renderer, with seeded actual DOM rows.
- Checks linear returned-node counts at 100, 500 and 1,000 saved rows (both
  user-only and mixed histories), without flaky elapsed-time assertions.
- Checks unchanged terminal-card replay.
- Checks duplicate IDs within a batch, equal text with distinct IDs, and
  detached rows after clearing the container.
- Existing cases cover SSE/history ordering, stale responses, pruning, session
  isolation and complete assistant text alongside tool calls.

All of these scripts passed after the application change:

```sh
node scripts/test_chat_history.js
node scripts/test_chat_terminal.js
node scripts/test_chat_audio.js
node scripts/test_memory_ui.js
node scripts/test_browser_mic.js
node tests/test_browser_mic.js
node tests/test_browser_wav.js
node scripts/test_ui_static.js
node scripts/test_llm_stream.js
node tests/test_chat_commands.js
node --check static/app.js
git diff --check
```

Browser prerequisites: Node, Playwright Chromium, its shared libraries, and a
working fontconfig configuration with fonts. This minimal container initially
lacked libraries/fonts. Tests ran after supplying them under `/tmp`, using
`LD_LIBRARY_PATH` and `FONTCONFIG_FILE` only for test processes. Missing fonts
caused baseline dashboard navigation timeouts; application code was unchanged
when that environment problem was resolved.

To rerun the regression against old application assets, export baseline
`static/` files to a separate directory and run:

```sh
PRAXIS_STATIC_DIR=/path/to/baseline/static CASE=history-refresh-linear \
  node scripts/test_chat_history.js
```

## Separate baseline test defect

`scripts/test_ui_browser.js` timed out waiting for tools on both this patch and
unchanged baseline assets. Its fixture serves `/api/tools`, whereas `loadTools`
requests `/api/tools/all`. Tracked separately as upstream/fork #7; not treated as
an application regression or silently counted as a passing test.

## Remaining limits

- Cold history insertion still does per-row ordering/layout work; not optimized
  or claimed linear by this patch.
- Full-history network transfer/JSON parsing is unchanged.
- This is not list virtualization or a bounded-history implementation.
- TTS-heavy history and repeatedly reused provider call IDs can have separate
  costs; the assertion covers the specified fixtures, not every possible input.
- No Rust/TUI changes or Cargo tests in this patch. No live deployment/restart.
