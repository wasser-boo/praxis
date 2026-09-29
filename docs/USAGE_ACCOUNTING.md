# Provider usage integrity — prerequisite for issue #4

Tracking: [upstream #4](https://forgejo.the.grid/Marvin/praxis/issues/4),
[fork #4](https://forgejo.the.grid/Praxis/praxis/issues/4).
Baseline: `testing` at `00f16ab` (selection/copy PR #10 merged).

## Scope and remaining work

This change fixes **incorrect provider usage data**, not the complete token/rate
feature. Issue #4 remains open. No counters are added to the TUI/dashboard yet.
Inspection found that message persistence drops `ChatResponse.usage`, the client
history/events do not carry it, and provider-generation duration is not persisted.
Those are separate follow-up changes requiring end-to-end regressions.

Do not reuse the dashboard history endpoint's `total_tokens` as model usage:
that field is an estimated transcript/context size. Do not count SSE characters
as tokens or divide by whole agent runtime (which includes tools and waiting).

## Investigation log: hypothesis → test → evidence → conclusion

All five initial regressions were added and run **before production changes**.
`cargo test --locked usage_metrics -- --test-threads=1` on the baseline:
**0 passed, 5 failed**. Fixtures use synthetic responses, loopback HTTP and the
real adapters/router; no account, provider credentials or live model is used.

1. **Missing or invalid counters become fabricated reported counts.**
   Test actual adapters with missing, empty, partial, negative, string, overflow,
   explicit zero and complete usage. OpenAI-compatible adapters turn missing
   fields into zero; `4294967296` wraps to zero. Codex/llama.cpp streaming and
   Ollama clamp oversized values. Confirmed: copied `unwrap_or(0) as u32` or
   saturating/clamping code destroys the unknown/zero distinction.

   Fix: a shared checked parser requires both exact nonnegative integer counts.
   Reject an unavailable/unrepresentable sample as `None`, without rejecting the
   otherwise valid reply. Explicit zero stays zero. Derive absent total by exact
   checked addition, never a text-based estimate. OpenAI/Responses preserve a
   valid supplied total if it is at least the input/output sum. Invalid or
   inconsistent totals make the sample unavailable.

   Retest after this change: **2 passed, 3 failed**; HTTP failures are now limited
   to the untouched Anthropic path. Codex and llama.cpp streaming regressions pass.

2. **Anthropic total/cache accounting is incorrect.**
   Test input=7, output=5, cache creation=11, cache read=13, including a nested
   cache-creation breakdown. Actual input=7 / total=0; expected input=31 / total=36.
   Anthropic's primary documentation explicitly defines total input as
   `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`:
   <https://platform.claude.com/docs/en/build-with-claude/prompt-caching>
   (read during this investigation, 2026-09-29).

   Fix: require the base input/output pair, add the disjoint top-level cache
   counts once with checked arithmetic, do not double-count the duration-specific
   nested breakdown. Absent optional cache fields contribute nothing; present
   malformed fields make usage unavailable. Shared Anthropic parsing also covers
   MiniMax/MiMo's Anthropic modes.

   Retest: **4 passed, 1 failed**; only continuation aggregation remains red.

3. **A missing continuation sample is silently omitted from a complete-looking sum.**
   Script two validated provider attempts, one reasoning continuation and one
   final reply. Either the first or last has no usage. Actual returned usage is
   the other sample (7/5/12), incorrectly implying a full logical-call total.

   Fix: initialize a fresh accumulator with the additive zero identity and make
   missing/overflowed usage sticky for the remainder of that logical call.
   Complete continuation samples are added exactly once. Arithmetic overflow
   returns unavailable rather than a saturated count. Failed retries continue
   not to contribute to validated-response usage; this is not a billing ledger.

   Retest: **5 passed, 0 failed** for the original regressions.

## Contract and limitations

- The existing `Option<Usage>` interface represents a **complete sample** or
  unavailable. Partial known fields are deliberately not exposed as zero. A
  future per-field optional schema can retain partial information if desired.
- Existing `u32` fields are retained; counts/sums outside that range are unavailable,
  not truncated, wrapped, or clamped. No storage/API schema migration here.
- Computed totals are exact sums of provider counters, not tokenizer estimates.
- OpenAI cached/reasoning detail counters are subsets and are not added again.
  Anthropic cache counters are disjoint and are added as documented.
- Router totals cover validated continuation steps of the returned logical call.
  Failed/discarded attempts and unrelated tool-loop turns are not billing totals.
- Missing usage leaves rate-limiter reservations at the existing conservative
  estimate; exact samples reconcile through the existing mechanism. The retry,
  cancellation, timeout and tool-execution policy is unchanged.
- A repeated streaming usage snapshot replaces the prior snapshot, not adds to
  it. History/session aggregation and reconnect deduplication still need their
  own persistent identity-based design and tests in the remainder of #4.

## Verification and regression prevention

```sh
cargo test --locked usage_metrics -- --test-threads=1
cargo test --locked gateway::llm:: -- --test-threads=1
cargo test --locked tui:: -- --test-threads=1
cargo test --locked sse:: -- --test-threads=1
node scripts/test_chat_terminal.js
node scripts/test_chat_history.js
node scripts/test_ui_browser.js
node scripts/test_ui_static.js
git diff --check
```

Nine named usage regressions cover the real HTTP adapter matrix, shared
Anthropic cache accounting, absent/invalid/overflow/zero counters, exact totals,
Codex and llama.cpp streaming, repeated usage snapshots, Ollama native streaming,
missing first/middle/last continuation samples, fully reported continuations,
aggregate overflow and failed-retry exclusion in both router entry modes.
Existing native Codex continuation tests independently check total 36 for three
reported 7/5/12 attempts.

No service restart, live deployment, real provider call or clipboard access is
part of this change. The full application-wide suite and live-provider smoke
tests are not claimed.

Final verification:

- **9 usage regressions passed** (also included in the provider suite).
- **128 provider/router tests passed**, 3 explicitly ignored (one live Codex
  test and two external-POML integration tests); no failures.
- **47 TUI + 2 shared SSE tests passed**.
- Normal debug binary built successfully (existing unrelated compiler warnings).
- Real-binary PTY startup/local SQLite/remote HTTP selection-copy smoke tests
  passed using captured output, preserving PR #10 behavior.
- Browser terminal/history/linear-refresh, six-viewport/theme UI and static UI
  suites passed. `git diff --check` passed.

Additional build/PTY commands used:

```sh
cargo build --locked --bin praxis
node scripts/test_tui_selection_pty.js target/debug/praxis
```
