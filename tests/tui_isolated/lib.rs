#![allow(dead_code, unused_imports)]
// Isolation harness: real TUI, SSE, and Message types. Only unrelated backend
// services are stubbed; unsupported operations panic instead of faking success.
#[path="__ROOT__/src/tui/mod.rs"]
mod tui;
#[path="__ROOT__/src/sse.rs"]
mod sse;
mod db {
    pub mod messages { include!(concat!(env!("OUT_DIR"), "/messages.rs")); }
    pub struct Database;
    impl Database {
        pub fn new(_: &std::path::Path) -> anyhow::Result<Self> { Ok(Self) }
        pub fn get_messages(&self,_: &str,_: usize)->anyhow::Result<Vec<messages::Message>> { unimplemented!("DB outside isolation scope") }
        pub fn clear_messages(&self,_: &str)->anyhow::Result<()> { unimplemented!() }
        pub fn merge_context(&self,_: &str,_: serde_json::Value)->anyhow::Result<()> { unimplemented!() }
        pub fn fork_context(&self,_: &str,_: &str,_: Option<&str>)->anyhow::Result<()> { unimplemented!() }
        pub fn delete_context(&self,_: &str)->anyhow::Result<()> { unimplemented!() }
    }
    pub mod secrets {
        pub struct Secrets { pub gateway_api_key: Option<String> }
        pub fn get_secrets()->Secrets { unimplemented!() }
    }
}
mod config {
    pub struct Config {pub data_dir:String,pub gateway_api_key:String,pub gateway_port:u16}
    impl Config {pub fn from_env()->Self {unimplemented!()}}
}
mod context_cmd {
    pub fn parse(_: &str)->anyhow::Result<()> {unimplemented!()}
    pub fn apply(_: &crate::db::Database,_: &str,_: &())->String {unimplemented!()}
}
mod gateway {
    pub mod llm {pub mod error {
        pub struct ProviderError;
        impl ProviderError {pub fn from_reqwest(e:reqwest::Error)->anyhow::Error {e.into()}}
    }}
    pub mod delegation {
        pub struct Delegation {pub id:String,pub status:String,pub task:String,pub result:Option<String>}
        pub fn list_delegations(_: &crate::db::Database,_: &str)->anyhow::Result<Vec<Delegation>> {unimplemented!()}
    }
}
