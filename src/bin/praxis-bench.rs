//! praxis-bench — Tool-Use-Zuverlässigkeits-Bench für lokale llama.cpp-Modelle.
//!
//! Misst über den ECHTEN praxis-Provider-Pfad (LlamaCppProvider, gleiches
//! Request-Format wie der Agent-Loop), wie zuverlässig ein Modell Tools wählt,
//! Schemas einhält, aus Fehlermeldungen recovert und Schweigen aushält:
//!
//!   1. single    — ein Tool-Call, korrektes Tool + gültige Args
//!   2. distract  — Ablenker-Tools: nah verwechselbare Tools richtig wählen
//!   3. chain     — 2-Schritt-Kette: erst Ergebnis von A, dann B damit
//!   4. recover   — Server-Feedback mit Schema-Fehler: Selbstkorrektur
//!   5. restraint — Antwort ohne Tool, wenn kein Tool nötig ist
//!
//! Alles lokal (LLAMACPP_API_BASE), deterministische Mock-Ergebnisse, 0 €.
//! Usage:
//!   cargo run --release --bin praxis-bench -- [--base http://127.0.0.1:11434] \
//!       [--model qwen3.6-35b-a3b] [--rounds 5] [--json]
//! Voraussetzung: laufender llama-server mit Tool-Support (chat template).

use praxis::gateway::llm::llamacpp::LlamaCppProvider;
use praxis::gateway::llm::provider::{ChatMessage, ChatRequest, FunctionDefinition, LLMProvider, ToolDefinition};
use serde_json::json;
use std::time::Instant;

// ---------------------------------------------------------------- Fixtures

fn tools() -> Vec<ToolDefinition> {
    let def = |name: &str, desc: &str, props: serde_json::Value, req: Vec<&str>| ToolDefinition {
        tool_type: "function".into(),
        function: FunctionDefinition {
            name: name.into(),
            description: desc.into(),
            parameters: json!({ "type": "object", "properties": props, "required": req }),
        },
    };
    vec![
        def("get_time",
            "Returns the current time as ISO-8601. Takes NO arguments.",
            json!({}), vec![]),
        def("calc",
            "Calculates two numbers: add, sub, mul or div.",
            json!({ "a": {"type":"number"}, "b": {"type":"number"}, "op": {"type":"string","enum":["add","sub","mul","div"]} }),
            vec!["a", "b", "op"]),
        def("read_file",
            "Reads a text file from the sandbox filesystem.",
            json!({ "path": {"type":"string"} }), vec!["path"]),
        def("list_dir",
            "Lists directory entries. Use for folders, NOT file contents.",
            json!({ "path": {"type":"string"} }), vec!["path"]),
        def("web_search",
            "Searches the web. Requires a query string.",
            json!({ "query": {"type":"string"} }), vec!["query"]),
        def("send_message",
            "Sends a message to a person by name.",
            json!({ "to": {"type":"string"}, "text": {"type":"string"} }), vec!["to", "text"]),
    ]
}

/// Deterministischer Mock-Executor: validiert Args, liefert feste Ergebnisse.
/// Rückgabe: (json-ergebnis, ok)
fn run_tool(name: &str, raw_args: &str) -> (String, bool) {
    let args: serde_json::Value = match serde_json::from_str(raw_args) {
        Ok(v) => v,
        Err(e) => return (format!("error: arguments are not valid JSON ({e}); resend the tool call with a JSON object as arguments"), false),
    };
    let arg_str = |k: &str| args.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    let arg_num = |k: &str| args.get(k).and_then(|v| v.as_f64());
    match name {
        "get_time" => (json!({ "time": "2026-09-21T18:04:00+02:00" }).to_string(), true),
        "calc" => match (arg_num("a"), arg_num("b"), arg_str("op")) {
            (Some(a), Some(b), Some(op)) => {
                let r = match op.as_str() {
                    "add" => a + b, "sub" => a - b, "mul" => a * b, "div" if b != 0.0 => a / b,
                    _ => return (format!("error: unknown op {op:?}; use add|sub|mul|div"), false),
                };
                (json!({ "result": r }).to_string(), true)
            }
            _ => (r#"error: calc requires numeric "a", numeric "b" and string "op""#.into(), false),
        },
        "read_file" => match arg_str("path") {
            Some(p) if p.ends_with(".txt") => (json!({ "content": "sandbox file: hello praxis bench" }).to_string(), true),
            Some(p) => (format!("error: file {p} not found; the sandbox only contains notes.txt"), false),
            None => (r#"error: read_file requires string "path""#.into(), false),
        },
        "list_dir" => match arg_str("path") {
            Some(_) => (json!({ "entries": ["notes.txt", "assets/"] }).to_string(), true),
            None => (r#"error: list_dir requires string "path""#.into(), false),
        },
        "web_search" => match arg_str("query") {
            Some(q) => (json!({ "results": [format!("result-for: {q}")] }).to_string(), true),
            None => (r#"error: web_search requires string "query""#.into(), false),
        },
        "send_message" => match (arg_str("to"), arg_str("text")) {
            (Some(to), Some(t)) => (json!({ "delivered": true, "to": to, "chars": t.len() }).to_string(), true),
            _ => (r#"error: send_message requires string "to" and string "text""#.into(), false),
        },
        other => (format!("error: unknown tool {other}"), false),
    }
}

fn sys_prompt() -> String {
    "You are a precise tool-using assistant. Call the single most fitting tool \
     with valid JSON arguments when the task needs one. If the task needs no \
     tool, answer directly without calling any tool. If a tool returns an \
     error, fix your arguments and call it again."
        .into()
}

// ---------------------------------------------------------------- Cases

struct CaseResult {
    id: &'static str,
    group: &'static str,
    first_try: bool,
    selection_ok: bool,
    schema_ok: bool,
    rounds: u32,
    answered_text: bool,
}

fn user_case(id: &'static str, group: &'static str, prompt: &str) -> (String, CaseResult) {
    (prompt.into(), CaseResult { id, group, first_try: false, selection_ok: false, schema_ok: false, rounds: 0, answered_text: false })
}

/// Ein Fall: bis max_rounds Tool-Runden + finale Antwort.
async fn run_case(
    provider: &dyn LLMProvider,
    case_id: &'static str,
    group: &'static str,
    prompt: &str,
    expect_tools: &[&str],          // erwartete Tool-Abfolge (Präfix reicht)
    max_rounds: u32,
) -> CaseResult {
    let mut r = CaseResult { id: case_id, group, first_try: false, selection_ok: false, schema_ok: false, rounds: 0, answered_text: false };
    let mut messages = vec![
        ChatMessage { role: "system".into(), content: Some(sys_prompt()), reasoning_content: None, content_parts: None, tool_calls: None, tool_call_id: None, tool_name: None },
        ChatMessage { role: "user".into(), content: Some(prompt.into()), reasoning_content: None, content_parts: None, tool_calls: None, tool_call_id: None, tool_name: None },
    ];
    let mut calls: Vec<String> = Vec::new();
    let mut all_schema_ok = true;
    let mut first_try_done = false;
    for _ in 0..max_rounds {
        let req = ChatRequest {
            messages: messages.clone(),
            tools: Some(tools()),
            temperature: Some(0.0),
            max_tokens: Some(512),
            model: None,
            vision_provider: None,
            vision_model: None,
            thinking: None,
        };
        let resp = match provider.chat(req).await {
            Ok(x) => x,
            Err(_) => return r,
        };
        let tcs = resp.tool_calls.clone().unwrap_or_default();
        if tcs.is_empty() {
            r.answered_text = resp.content.as_deref().map(|c| !c.trim().is_empty()).unwrap_or(false);
            if !first_try_done {
                r.first_try = r.answered_text && expect_tools.is_empty();
                first_try_done = true;
            }
            break;
        }
        // Mehr als 1 Call pro Runde = schema-discipline-Verlust, wir nehmen
        // den ersten und behandeln den Rest als Auswahl-Fehler (nicht validierbar
        // in einem sequentiellen Mock).
        if tcs.len() > 1 {
            all_schema_ok = false;
        }
        let tc = &tcs[0];
        calls.push(tc.function.name.clone());
        r.rounds += 1;
        let (result, ok) = run_tool(&tc.function.name, &tc.function.arguments);
        if !ok { all_schema_ok = false; }
        let mut assistant = ChatMessage { role: "assistant".into(), content: resp.content.clone(), reasoning_content: None, content_parts: None, tool_calls: None, tool_call_id: None, tool_name: None };
        assistant.tool_calls = resp.tool_calls.clone();
        messages.push(assistant);
        messages.push(ChatMessage {
            role: "tool".into(),
            content: Some(result),
            reasoning_content: None,
            content_parts: None,
            tool_calls: None,
            tool_call_id: Some(tc.id.clone()),
            tool_name: Some(tc.function.name.clone()),
        });
        if !first_try_done {
            r.first_try = ok && calls.as_slice() == expect_tools;
            first_try_done = true;
        }
        if r.rounds >= max_rounds { break; }
    }
    // Auswahl: erwartete Abfolge als PRÄFIX der getätigten Calls (Kette darf
    // danach noch aufräumen), plus Abschließende Textantwort bei Tool-Fällen.
    r.selection_ok = !expect_tools.is_empty()
        && calls.len() >= expect_tools.len()
        && calls.iter().zip(expect_tools.iter()).all(|(a, e)| a == e);
    r.schema_ok = all_schema_ok && (calls.is_empty() == expect_tools.is_empty());
    if !expect_tools.is_empty() {
        r.first_try = r.first_try && r.answered_text;
    }
    r
}

// ---------------------------------------------------------------- Main

#[tokio::main]
async fn main() {
    let base = std::env::var("PRAXIS_BENCH_BASE")
        .ok()
        .or_else(|| std::env::var("LLAMACPP_API_BASE").ok())
        .unwrap_or_else(|| "http://127.0.0.1:11434".into());
    let model = std::env::var("PRAXIS_BENCH_MODEL")
        .ok()
        .or_else(|| std::env::var("LLAMACPP_MODEL").ok())
        .unwrap_or_else(|| "qwen3.6-35b-a3b".into());
    let json_out = std::env::args().any(|a| a == "--json");

    println!("praxis-bench — Tool-Use-Reliability");
    println!("  provider: llamacpp @ {base}  model: {model}");
    println!("  (llama-server mit Tool-Support muss dort laufen)\n");

    let provider = LlamaCppProvider::new(None, model.clone(), base);
    let t0 = Instant::now();

    let mut cases: Vec<(&'static str, &'static str, String, Vec<&'static str>, u32)> = vec![
        // --- single: korrektes Tool + Args
        ("s1", "single", "Wie viel ist 17 * 24? Benutze den Rechner.".into(), vec!["calc"], 2),
        ("s2", "single", "Lies den Inhalt von notes.txt".into(), vec!["read_file"], 2),
        ("s3", "single", "Was steht in der Uhrzeit gerade? Frag das Time-Tool".into(), vec!["get_time"], 2),
        ("s4", "single", "Suche im Web nach 'qwen3 a3b benchmark'".into(), vec!["web_search"], 2),
        ("s5", "single", "Schick Max die Nachricht: 'Bench läuft'".into(), vec!["send_message"], 2),
        ("s6", "single", "Was liegt im Sandbox-Verzeichnis?".into(), vec!["list_dir"], 2),
        // --- distract: nah benachbarte Tools, Verwechslungsfallen
        ("d1", "distract", "Lies die Datei notes.txt (NICHT das Verzeichnis auflisten!)".into(), vec!["read_file"], 2),
        ("d2", "distract", "Gib mir die Ordner-Übersicht (NICHT den Dateiinhalt!)".into(), vec!["list_dir"], 2),
        ("d3", "distract", "Wie spät ist es? (Uhrzeit, nicht Datum rechnen)".into(), vec!["get_time"], 2),
        ("d4", "distract", "Schreibe an Lisa: 'komm um 18 Uhr' — sende, suche nicht im Web".into(), vec!["send_message"], 2),
        // --- chain: erst Info holen, dann damit weiterarbeiten
        ("c1", "chain", "Rechne: die Uhrzeit-Stunde mal 2. (Erst Uhrzeit holen, dann rechnen)".into(), vec!["get_time", "calc"], 3),
        ("c2", "chain", "Lies notes.txt und schick den Inhalt per Nachricht an Anna".into(), vec!["read_file", "send_message"], 4),
        // --- recover: Fehler zurückspielen → Korrektur
        ("r1", "recover", "Rechne 9 div 0 und sag mir das Ergebnis".into(), vec!["calc", "calc"], 4),
        ("r2", "recover", "Lies die Datei log.dat (falls das fehlschlägt, lies notes.txt)".into(), vec!["read_file", "read_file"], 4),
        // --- restraint: KEIN Tool rufen
        ("n1", "restraint", "Erkläre kurz, was ein A3B-MoE-Modell ist. Kein Tool nötig.".into(), vec![], 1),
        ("n2", "restraint", "Sag 'Bench fertig' — benutze dafür KEIN Werkzeug".into(), vec![], 1),
    ];

    let mut results = Vec::new();
    for (id, group, prompt, expect, rounds) in cases.drain(..) {
        let r = run_case(&provider, id, group, &prompt, &expect, rounds).await;
        if !json_out {
            let status = if r.first_try { "✓ first" } else if r.selection_ok && r.schema_ok { "~ ok" } else { "✗" };
            println!("{status} [{group:>9}] {id}: rounds={} selection={} schema={} text={}", r.rounds, r.selection_ok, r.schema_ok, r.answered_text);
        }
        results.push(r);
    }
    let secs = t0.elapsed().as_secs_f32();

    let stat = |g: &str| -> (usize, usize, usize, usize, f32) {
        let rs: Vec<&CaseResult> = results.iter().filter(|r| r.group == g).collect();
        let n = rs.len().max(1);
        let first = rs.iter().filter(|r| r.first_try).count();
        let sel = rs.iter().filter(|r| r.selection_ok).count();
        let sch = rs.iter().filter(|r| r.schema_ok).count();
        (first, sel, sch, rs.len(), (first as f32 / n as f32) * 100.0)
    };

    if json_out {
        let summary = serde_json::json!({
            "model": model,
            "total_first_try": results.iter().filter(|r| r.first_try).count(),
            "total": results.len(),
            "groups": {
                "single":    { "first_try": stat("single").0,    "selection": stat("single").1,    "schema": stat("single").2,    "n": stat("single").3 },
                "distract":  { "first_try": stat("distract").0,  "selection": stat("distract").1,  "schema": stat("distract").2,  "n": stat("distract").3 },
                "chain":     { "first_try": stat("chain").0,     "selection": stat("chain").1,     "schema": stat("chain").2,     "n": stat("chain").3 },
                "recover":   { "first_try": stat("recover").0,   "selection": stat("recover").1,   "schema": stat("recover").2,   "n": stat("recover").3 },
                "restraint": { "first_try": stat("restraint").0, "selection": stat("restraint").1, "schema": stat("restraint").2, "n": stat("restraint").3 },
            },
            "wall_secs": secs,
        });
        println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    } else {
        println!("\n──────────────────────────────────────────────");
        println!("Gruppe      first-try  Auswahl  Schema  N   Anteil");
        for g in ["single", "distract", "chain", "recover", "restraint"] {
            let (f, s, c, n, pct) = stat(g);
            println!("{g:<10}  {f:>5}/{n:<2}  {s:>4}/{n:<2}  {c:>4}/{n:<2}  {n:>2}  {pct:5.1}%");
        }
        let first_total = results.iter().filter(|r| r.first_try).count();
        println!("──────────────────────────────────────────────");
        println!("GESAMT: {first_total}/{} first-try ({:.0}%) · {:.1}s · {model}",
                 results.len(), (first_total as f32 / results.len() as f32) * 100.0, secs);
        println!("\nInterpretation: >=80% = Alltagstauglich · 60-80% = mit Retry-Guardrails ok · <60% = nur mit Decision-Routing");
    }
}
