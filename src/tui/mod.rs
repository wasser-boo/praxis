//! Terminal-UI chat client for Praxis.
//!
//! Architecture:
//!
//! ```text
//!     ┌─────────────────┐                 ┌────────────────────┐
//!     │  praxis chat    │  HTTP POST   ──▶│  gateway (3537)    │
//!     │  (this module)  │   (send msg)    │  /v1/chat          │
//!     │                 │                 │                    │
//!     │  Read DB ◀──────┼──────  SQLite ──┤  agent_loop writes │
//!     │  (history,      │                 │  to messages table │
//!     │   sessions)     │                 │                    │
//!     └─────────────────┘                 └────────────────────┘
//! ```
//!
//! - History is read directly from the local SQLite database, so it
//!   persists across runs and is shared with the web dashboard.
//! - Sessions follow the same model as the web chat: each session is
//!   its own `user_id` at the storage layer; new sessions are forked
//!   from the parent context so they inherit settings but start with
//!   an empty message log.
//! - The agent loop is invoked over HTTP to the local gateway. The
//!   TUI subscribes automatically to authenticated gateway SSE for text,
//!   reasoning and tool-call fragments. SQLite polling reconciles durable
//!   results and provides history/reconnect recovery.

mod app;
mod streaming;
mod remote;
mod sessions;
mod ui;
mod tool_result;

pub use app::run as run_chat;
