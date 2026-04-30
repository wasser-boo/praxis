# AGENTS.md — Praxis AI Agent Platform

> **Lies zuerst `docs/BLUEPRINT.md` — dort steht ALLES.**

---

## Schnellstart

```
1. Lese docs/BLUEPRINT.md (die EINE Datei die alles enthält)
2. Reference-Code liegt in reference/src/ (nur zum Nachschlagen)
3. Implementiere Phase für Phase (TDD: Tests zuerst)
```

## Projektstruktur

```
praxis/
├── Cargo.toml                    # Dependencies (fertig)
├── src/                          # HIER WIRD IMPLEMENTIERT
│   ├── main.rs                   # CLI + Service-Start
│   ├── lib.rs                    # Module exports
│   ├── config.rs                 # Configuration
│   ├── cl.rs                     # Context Language Parser
│   ├── tags.rs                   # §-Tag Parser
│   ├── event_channel.rs          # Event Bus
│   ├── gateway/                  # Core Gateway
│   │   ├── mod.rs                # Axum Server
│   │   ├── auth.rs               # JWT + API-Key
│   │   ├── rate_limiter.rs       # Rate Limiting
│   │   ├── ws_handler.rs         # WebSocket
│   │   ├── http_handler.rs       # REST API
│   │   ├── message_handler.rs    # Message Pipeline
│   │   ├── agent_loop.rs         # Agent Loop
│   │   ├── llm/                  # LLM Router
│   │   │   ├── mod.rs
│   │   │   ├── provider.rs       # Provider Trait
│   │   │   ├── openai.rs
│   │   │   ├── anthropic.rs
│   │   │   ├── ollama.rs
│   │   │   ├── minimax.rs
│   │   │   └── mimo.rs
│   │   ├── poml.rs               # POML Renderer
│   │   └── templates.rs          # Template Management
│   ├── db/                       # SQLite Database
│   │   ├── mod.rs
│   │   ├── contexts.rs
│   │   ├── messages.rs
│   │   ├── memory.rs
│   │   ├── secrets.rs
│   │   ├── pairings.rs
│   │   ├── templates.rs
│   │   ├── enc2.rs
│   │   └── logs.rs
│   ├── discord/                  # Discord Bot
│   │   ├── mod.rs
│   │   ├── handler.rs
│   │   ├── commands.rs
│   │   └── ws_client.rs
│   ├── tools/                    # Agent Tools
│   │   ├── mod.rs
│   │   ├── execute_terminal.rs
│   │   ├── write_file.rs
│   │   ├── edit_file.rs
│   │   ├── agent_control.rs
│   │   ├── context_tools.rs
│   │   ├── get_context.rs
│   │   ├── discord_upload.rs
│   │   ├── discord_send_message.rs
│   │   └── mcp.rs
│   ├── voice/                    # Voice (TTS/STT)
│   │   ├── mod.rs
│   │   └── handler.rs
│   ├── dashboard/                # Web Dashboard
│   │   ├── mod.rs
│   │   └── routes.rs
│   ├── skills/                   # Skills System (NEU)
│   │   ├── mod.rs
│   │   └── executor.rs
│   └── plugins/                  # Plugin System (NEU)
│       └── mod.rs
├── docs/
│   ├── AGENTS.md                 # Diese Datei
│   └── BLUEPRINT.md              # MASTER BLUEPRINT (alles drin)
├── reference/                    # ALTER CODE (nur Referenz)
│   └── src/                      # Kopie des alten Projekts
├── templates/                    # POML Templates
├── contexts/                     # CL Workflows
├── migrations/                   # SQL Migrations
├── skills/                       # User Skills
└── plugins/                      # Plugin-Definitionen
```

## Regeln

1. **IMMER `docs/BLUEPRINT.md` zuerst lesen** — dort steht alles
2. **TDD**: Tests schreiben BEVOR du implementierst
3. **Security First**: Keine `unwrap()` auf User-Input, keine Secrets im Log
4. **Reference-Code ist nur Referenz** — implementiere NEU in `src/`
5. **Jede Phase**: Tests schreiben → Implementieren → Tests grün → Refactorn

## Phasen

| Phase | Was | Tests |
|-------|-----|-------|
| 0 | Security + Bugfixes | `security_tests` |
| 1 | Database (SQLite) | `db_tests` |
| 2 | Gateway + LLM Router | `gateway_tests` |
| 3 | Tools | `tool_tests` |
| 4 | Discord | `discord_tests` |
| 5 | Voice | `voice_tests` |
| 6 | Dashboard | `dashboard_tests` |
| 7 | Skills + Plugins | `plugin_tests` |
| 8 | RAG + Web Search | `rag_tests` |
| 9 | Cron Jobs | `cron_tests` |
| 10 | Integration | `integration_tests` |

## Commands

```bash
# Tests
cargo test
cargo test --lib
cargo test --test '*'

# Build
cargo build --release
cargo build --release --features songbird

# Run
cargo run -- run
cargo run -- onboard --interactive
cargo run -- pair ABCD-1234
```
