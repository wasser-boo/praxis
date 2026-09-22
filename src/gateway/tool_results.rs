//! Common delivery policy for every gateway tool loop. Raw text is saved before
//! choosing what goes into the LLM context. Default is FULL text, not a preview;
//! legacy preview limits are ignored. Image content parts stay with the caller.
use crate::{db::Database, gateway::llm::provider::ToolCall, tools::tool_output};

pub fn prepare(db: &Database, user: &str, tool: &str, call: &str, text: &str) -> anyhow::Result<String> {
    if tool == "read_tool_result" {
        // This is already a selected view of an existing snapshot, not a new source.
        return Ok(text.to_string());
    }
    let output = db.save_tool_output(user, tool, call, text)?;
    Ok(tool_output::complete_response(&output))
}

pub fn prepare_for_call(db: &Database, user: &str, call: &ToolCall, text: &str) -> anyhow::Result<String> {
    let args: serde_json::Value = serde_json::from_str(&call.function.arguments)?;
    let selection = args.get("_output").filter(|_| call.function.name != "read_tool_result");
    let Some(selection) = selection else {
        return prepare(db, user, &call.function.name, &call.id, text);
    };
    let output = db.save_tool_output(user, &call.function.name, &call.id, text)?;
    match tool_output::selected_response(&output, selection) {
        Ok(selected) => Ok(selected),
        Err(error) => Ok(serde_json::json!({
            "output_id":output.id, "error":format!("Output selection unavailable: {error}"),
            "tool_executed":true, "note":"The original tool already executed. Use read_tool_result with a corrected view; do not rerun it."
        }).to_string()),
    }
}
