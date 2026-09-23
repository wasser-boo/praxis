# TODO — Praxis Local Model Reliability

## Completed ✅
- [x] Real Praxis pipeline (Gateway → POML → history/tools → llama.cpp/pgpu)
- [x] Native SSE streaming + thinking display (`/show_thinking`, Discord codeboxes)
- [x] `/stop` command, session renaming (TUI/Web)
- [x] 500-task benchmark (292/500 running)
- [x] Decision endpoint activated (qwen3-1.7B, `--decision-seqs 3`)
- [x] Configurable tag prefix (`settings.tag_prefix`)
- [x] Free router integration (`settings.use_freerouter`)
- [x] Decision routing (pre-request, CAS commit, failure-safe)
- [x] 20-task experiment with 4 arms (5/20 tasks done, decision arm works)
- [x] Context compaction before requests (shared service)

## In Progress 🔄
- [ ] Complete 20-task × 4-arm experiment (15 tasks remaining: 06-20)
- [ ] Run decision arm on all 20 tasks

## Not Started ❌
### Trusted-Template Lifecycle Actions
- [ ] `PREFIX/clearmessagesholdimportantones` — classify older content good/bad for relevance
- [ ] `PREFIX/forgetallthistemplaterememberallafterthat` — template/state context boundary
- [ ] Trusted-template authorization (not user/model text)
- [ ] Per-message good/bad labels
- [ ] Whole-turn/tool-group protection
- [ ] Template-entry watermark boundary
- [ ] Atomic apply/rollback, cancellation, audit/restore

### OpenAPI/API Explorer + Scoped/Expiring/Revocable API Keys
- [ ] OpenAPI spec generation (Praxis + pgpu)
- [ ] API explorer UI in dashboard
- [ ] Key hashing, expiry, revocation, resource scopes
- [ ] Server-side checks for indirect access (chat→tools→WS→proxy)
- [ ] No self-escalation by delegated keys
- [ ] pgpu integration (separate repo)

### Deployment & Validation
- [ ] Deploy updated Praxis binary
- [ ] Live Discord/TUI end-to-end validation
- [ ] Compaction non-default session atomicity audit
- [ ] Oversized-template fallback validation
- [ ] Archive artifacts outside `/tmp`

## Notes
- Decision server: `http://100.105.88.246:11440` (qwen3-1.7B, working)
- Chat model: `http://100.105.88.246:11435` (qwen3.8-27B, working)
- pgpu router: `http://100.105.88.246:8080` (slot 1 healthy)
- Benchmark artifacts: `/tmp/praxis-live-baseline-9uao6aup/`
- State experiment: `/tmp/praxis-live-baseline-9uao6aup/state-experiment-4arm/`