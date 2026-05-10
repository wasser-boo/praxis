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
//!   TUI then polls the messages table for new rows (~250 ms) and
//!   re-renders. We don't hook into the SSE stream because that would
//!   require the JWT-protected dashboard endpoint; polling the DB is
//!   simpler, fully offline, and plenty fast for a TUI.

mod app;
mod sessions;
mod ui;

pub use app::run as run_chat;
