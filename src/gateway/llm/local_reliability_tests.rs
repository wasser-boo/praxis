use super::*;
use crate::gateway::llm::provider::FunctionCall;

fn response(content: Option<&str>, reasoning: Option<&str>, finish: &str) -> ChatResponse {
    ChatResponse {
        content: content.map(String::from), reasoning_content: reasoning.map(String::from),
        tool_calls: None, finish_reason: Some(finish.into()), usage: None,
    }
}
#[test]
fn small_model_reasoning_is_not_a_final_answer_or_tool_history() {
    let r = response(None, Some("still thinking"), "length");
    assert_eq!(validate_response(r).unwrap_err().kind, ErrorKind::OutputLimit);
    assert_eq!(validate_response(response(None, Some("only reasoning"), "stop")).unwrap_err().kind, ErrorKind::InvalidResponse);
    let mut r = response(None, Some("tool reasoning"), "tool_calls");
    r.tool_calls = Some(vec![ToolCall { id: "a".into(), function: FunctionCall {name: "read_file".into(), arguments: "{}".into()} }]);
    let r = validate_response(r).unwrap();
    assert!(r.content.is_none());
    assert_eq!(r.reasoning_content.as_deref(), Some("tool reasoning"));
}
#[test]
fn small_model_cutoff_never_executes_a_partial_batch() {
    let mut r = response(Some("partial answer"), None, "length");
    r.tool_calls = Some(vec![
        ToolCall { id: "a".into(), function: FunctionCall {name: "write_file".into(), arguments: "{}".into()} },
        ToolCall { id: "b".into(), function: FunctionCall {name: "write_file".into(), arguments: "{\"path\":".into()} },
    ]);
    assert_eq!(validate_response(r).unwrap_err().kind, ErrorKind::OutputLimit);
}
#[test]
fn small_model_empty_tool_name_is_never_repaired_into_an_action() {
    let mut r = response(None, None, "tool_calls");
    r.tool_calls = Some(vec![ToolCall { id: "a".into(), function: FunctionCall {name: "".into(), arguments: "".into()} }]);
    assert_eq!(validate_response(r).unwrap_err().kind, ErrorKind::InvalidResponse);
}
