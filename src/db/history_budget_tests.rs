use super::*;

#[test]
fn small_model_history_keeps_complete_turns_at_1000_budget_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path()).unwrap();
    let user = "synthetic-history";
    db.save_context(&db.load_context(user).unwrap()).unwrap();
    for turn in 0..8 {
        db.add_message(user, &Message::user(format!("question {turn}"))).unwrap();
        db.add_message(user, &Message::assistant_with_tool_calls(String::new(), vec![ToolCallData {
            id: format!("call_{turn}"), function: FunctionCallData {name:"read_file".into(),arguments:"{}".into()},
        }])).unwrap();
        db.add_message(user, &Message::tool("synthetic 漢🙂 output\n".repeat(37), format!("call_{turn}"))).unwrap();
    }
    for budget in 0..1000 {
        let (messages, _) = db.get_messages_with_token_budget(user, budget).unwrap();
        assert_eq!(messages.first().unwrap().role, "user", "partial turn at budget {budget}");
        assert!(messages.iter().any(|m|m.content == "question 7"));
        for result in messages.iter().filter(|m|m.role == "tool") {
            assert!(messages.iter().filter_map(|m|m.tool_calls.as_ref()).flatten()
                .any(|call| Some(&call.id) == result.tool_call_id.as_ref()), "orphan at budget {budget}");
        }
    }
}
