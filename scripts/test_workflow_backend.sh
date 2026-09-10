#!/usr/bin/env bash
# Scoped Rust regressions: temporary databases/files and mock providers only.
# Requires the actual Microsoft POML CLI for explicitly included render tests.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${POML_CLI:?Set POML_CLI to the Microsoft JavaScript CLI}"
CARGO_BIN="${CARGO_BIN:-cargo}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"
for filter in \
  'assets::' 'sm::' 'context_cmd::' 'db::contexts::' 'db::memory::' 'db::tools::' 'skills::' \
  'gateway::agent_loop::' 'gateway::message_handler::' \
  'gateway::llm::skill_tool_loop_tests' 'gateway::poml::' 'gateway::templates::' \
  'gateway::prompt::' 'tools::update_template::' 'tools::context_tools::' \
  'tools::agent_control::' 'tools::get_context::' 'discord::commands::' \
  'dashboard::routes::' 'tui::ui::'; do
  printf '\nGROUP %s\n' "$filter"
  "$CARGO_BIN" test --offline --release \
    --config 'profile.release.package.praxis.opt-level=0' \
    --features songbird --lib "$filter" -- --include-ignored --test-threads=1
done
