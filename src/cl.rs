use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextLang {
    pub name: String,
    pub version: String,
    pub steps: Vec<String>,
    pub states: HashMap<String, ClState>,
    pub transitions: Vec<ClTransition>,
    pub auto_rules: Vec<ClAutoRule>,
    pub overrides: Vec<ClOverride>,
    #[serde(default)]
    pub secret_overrides: Vec<ClSecretOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClState {
    pub variables: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClTransition {
    pub from: String,
    pub to: String,
    pub condition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClAutoRule {
    pub condition: String,
    pub target_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClOverride {
    pub condition: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClSecretOverride {
    pub condition: String,
    pub key: String,
    pub value: String,
}

const ALLOWED_SECRET_FIELDS: &[&str] = &[
    "mimo_voice",
    "mimo_tts_type",
    "mimo_style_instruction",
    "mimo_voice_design_prompt",
    "minimax_voice_id",
    "minimax_model",
    "voice_elevenlabs_voice_id",
    "qwen_tts_speaker",
    "qwen_tts_language",
    "qwen_voice_clone_enabled",
    "qwen_voice_clone_audio_path",
    "qwen_voice_clone_prompt",
    "rvc_on",
];

#[derive(Debug)]
pub enum ClError {
    ParseError(String),
    IoError(std::io::Error),
}

impl std::fmt::Display for ClError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClError::ParseError(msg) => write!(f, "CL Parse Error: {}", msg),
            ClError::IoError(e) => write!(f, "CL IO Error: {}", e),
        }
    }
}

impl From<std::io::Error> for ClError {
    fn from(e: std::io::Error) -> Self {
        ClError::IoError(e)
    }
}

pub fn parse(content: &str) -> Result<ContextLang, ClError> {
    let mut cl = ContextLang::default();
    let mut current_section: Option<String> = None;
    let mut current_state_name: Option<String> = None;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];
            let parts: Vec<&str> = inner.splitn(2, ' ').collect();

            match parts[0] {
                "state" => {
                    let name = parts
                        .get(1)
                        .ok_or_else(|| {
                            ClError::ParseError(format!(
                                "Line {}: [state] requires a name",
                                line_num + 1
                            ))
                        })?
                        .to_string();
                    current_state_name = Some(name.clone());
                    cl.states.entry(name).or_default();
                    current_section = Some("state".to_string());
                }
                "transitions" => {
                    current_state_name = None;
                    current_section = Some("transitions".to_string());
                }
                "auto" => {
                    current_state_name = None;
                    current_section = Some("auto".to_string());
                }
                "overrides" => {
                    current_state_name = None;
                    current_section = Some("overrides".to_string());
                }
                "secrets" => {
                    current_state_name = None;
                    current_section = Some("secrets".to_string());
                }
                other => {
                    return Err(ClError::ParseError(format!(
                        "Line {}: Unknown section [{}]",
                        line_num + 1,
                        other
                    )));
                }
            }
            continue;
        }

        if trimmed.starts_with('@') {
            let (key, value) = parse_metadata(trimmed, line_num)?;
            match key.as_str() {
                "name" => cl.name = value,
                "version" => cl.version = value,
                "steps" => {
                    cl.steps = parse_array(&value);
                }
                _ => {}
            }
            continue;
        }

        match current_section.as_deref() {
            Some("state") => {
                if let Some(ref state_name) = current_state_name {
                    let (key, value) = parse_assignment(trimmed, line_num)?;
                    cl.states
                        .entry(state_name.clone())
                        .or_default()
                        .variables
                        .insert(key, value);
                }
            }
            Some("transitions") => {
                let transition = parse_transition(trimmed, line_num)?;
                cl.transitions.push(transition);
            }
            Some("auto") => {
                let rule = parse_auto_rule(trimmed, line_num)?;
                cl.auto_rules.push(rule);
            }
            Some("overrides") => {
                let overr = parse_override(trimmed, line_num)?;
                cl.overrides.push(overr);
            }
            Some("secrets") => {
                let secret_overr = parse_secret_override(trimmed, line_num)?;
                cl.secret_overrides.push(secret_overr);
            }
            _ => {
                if trimmed.contains('=') && !trimmed.contains("->") {
                    let (key, value) = parse_assignment(trimmed, line_num)?;
                    cl.states
                        .entry("_default".to_string())
                        .or_default()
                        .variables
                        .insert(key, value);
                }
            }
        }
    }

    Ok(cl)
}

fn parse_metadata(line: &str, _line_num: usize) -> Result<(String, String), ClError> {
    let rest = line.strip_prefix('@').unwrap();
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    let key = parts[0].to_string();
    let value = parts.get(1).unwrap_or(&"").trim().to_string();

    let value = if value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value
    };

    Ok((key, value))
}

fn parse_array(value: &str) -> Vec<String> {
    let inner = value.trim();
    if inner.starts_with('[') && inner.ends_with(']') {
        inner[1..inner.len() - 1]
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![inner.to_string()]
    }
}

fn parse_assignment(line: &str, line_num: usize) -> Result<(String, String), ClError> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(ClError::ParseError(format!(
            "Line {}: Expected 'key = value', got '{}'",
            line_num + 1,
            line
        )));
    }

    let key = parts[0].trim().to_string();
    let value = parts[1].trim().to_string();

    let value = if value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value
    };

    Ok((key, value))
}

fn parse_transition(line: &str, line_num: usize) -> Result<ClTransition, ClError> {
    let line = line.replace('→', "->");
    let parts: Vec<&str> = line.splitn(2, "->").collect();
    if parts.len() != 2 {
        return Err(ClError::ParseError(format!(
            "Line {}: Expected 'from -> to : when condition', got '{}'",
            line_num + 1,
            line
        )));
    }

    let from = parts[0].trim().to_string();
    let rest = parts[1].trim();

    let to_and_cond: Vec<&str> = rest.splitn(2, ':').collect();
    let to = to_and_cond[0].trim().to_string();
    let condition = if to_and_cond.len() > 1 {
        let cond = to_and_cond[1].trim();
        cond.strip_prefix("when ").unwrap_or(cond).to_string()
    } else {
        String::new()
    };

    Ok(ClTransition {
        from,
        to,
        condition,
    })
}

fn parse_auto_rule(line: &str, line_num: usize) -> Result<ClAutoRule, ClError> {
    let line = line.replace('→', "->");
    let parts: Vec<&str> = line.splitn(2, "->").collect();
    if parts.len() != 2 {
        return Err(ClError::ParseError(format!(
            "Line {}: Expected 'condition -> use state', got '{}'",
            line_num + 1,
            line
        )));
    }

    let condition = parts[0].trim().to_string();
    let rest = parts[1].trim();
    let target_state = rest.strip_prefix("use ").unwrap_or(rest).trim().to_string();

    Ok(ClAutoRule {
        condition,
        target_state,
    })
}

fn parse_override(line: &str, line_num: usize) -> Result<ClOverride, ClError> {
    let line = line.replace('→', "->");
    let parts: Vec<&str> = line.splitn(2, "->").collect();
    if parts.len() != 2 {
        return Err(ClError::ParseError(format!(
            "Line {}: Expected 'if condition -> key = value', got '{}'",
            line_num + 1,
            line
        )));
    }

    let condition = parts[0].trim();
    let condition = condition
        .strip_prefix("if ")
        .unwrap_or(condition)
        .trim()
        .to_string();

    let kv = parts[1].trim();
    let kv_parts: Vec<&str> = kv.splitn(2, '=').collect();
    if kv_parts.len() != 2 {
        return Err(ClError::ParseError(format!(
            "Line {}: Expected 'key = value' after ->, got '{}'",
            line_num + 1,
            kv
        )));
    }

    let key = kv_parts[0].trim().to_string();
    let value = kv_parts[1].trim();
    let value = if value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    };

    Ok(ClOverride {
        condition,
        key,
        value,
    })
}

fn parse_secret_override(line: &str, line_num: usize) -> Result<ClSecretOverride, ClError> {
    let line = line.replace('→', "->");
    let parts: Vec<&str> = line.splitn(2, "->").collect();
    if parts.len() != 2 {
        return Err(ClError::ParseError(format!(
            "Line {}: Expected 'if condition -> key = value', got '{}'",
            line_num + 1,
            line
        )));
    }

    let condition = parts[0].trim();
    let condition = condition
        .strip_prefix("if ")
        .unwrap_or(condition)
        .trim()
        .to_string();

    let kv = parts[1].trim();
    let kv_parts: Vec<&str> = kv.splitn(2, '=').collect();
    if kv_parts.len() != 2 {
        return Err(ClError::ParseError(format!(
            "Line {}: Expected 'key = value' after ->, got '{}'",
            line_num + 1,
            kv
        )));
    }

    let key = kv_parts[0].trim().to_string();
    let value = kv_parts[1].trim();
    let value = if value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    };

    if !ALLOWED_SECRET_FIELDS.contains(&key.as_str()) {
        return Err(ClError::ParseError(format!(
            "Line {}: Secret field '{}' is not allowed. Allowed: {:?}",
            line_num + 1,
            key,
            ALLOWED_SECRET_FIELDS
        )));
    }

    Ok(ClSecretOverride {
        condition,
        key,
        value,
    })
}

/// Apply CL workflow to context. Returns secret overrides.
pub fn apply_to_context(
    cl: &ContextLang,
    context: &mut serde_json::Value,
) -> Vec<(String, String)> {
    let obj = match context.as_object_mut() {
        Some(o) => o,
        None => return Vec::new(),
    };

    let active_state = obj
        .get("active_state")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Apply default state variables first
    if let Some(default_state) = cl.states.get("_default") {
        for (key, value) in &default_state.variables {
            if !obj.contains_key(key) {
                obj.insert(key.clone(), serde_json::Value::String(value.clone()));
            }
        }
    }

    // Auto-rules: always evaluate to allow automatic state transitions
    let resolved_state = resolve_auto_state(cl, obj, &active_state);

    // Apply state variables
    if let Some(state) = cl.states.get(&resolved_state) {
        for (key, value) in &state.variables {
            set_nested_value(context, key, serde_json::Value::String(value.clone()));
        }
        set_nested_value(
            context,
            "active_state",
            serde_json::Value::String(resolved_state),
        );
    }

    // Apply overrides
    for overr in &cl.overrides {
        if evaluate_condition(&overr.condition, context.as_object().unwrap_or(&serde_json::Map::new())) {
            set_nested_value(context, &overr.key, serde_json::Value::String(overr.value.clone()));
        }
    }

    // Collect secret overrides
    let mut secret_changes = Vec::new();
    for secret_overr in &cl.secret_overrides {
        if evaluate_condition(&secret_overr.condition, context.as_object().unwrap_or(&serde_json::Map::new())) {
            secret_changes.push((secret_overr.key.clone(), secret_overr.value.clone()));
        }
    }

    secret_changes
}

fn resolve_auto_state(
    cl: &ContextLang,
    context: &serde_json::Map<String, serde_json::Value>,
    current_state: &str,
) -> String {
    for rule in &cl.auto_rules {
        if evaluate_condition(&rule.condition, context) {
            return rule.target_state.clone();
        }
    }
    // Fall back to current state if set, otherwise first step or first state key
    if !current_state.is_empty() {
        return current_state.to_string();
    }
    cl.steps
        .first()
        .cloned()
        .or_else(|| cl.states.keys().next().cloned())
        .unwrap_or_default()
}

/// Evaluate a condition against context. Supports ==, !=, >, <, >=, <=, &&, ||.
pub fn evaluate_condition(
    condition: &str,
    context: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let condition = condition.trim();

    if condition.contains("&&") {
        return condition
            .split("&&")
            .all(|part| evaluate_condition(part.trim(), context));
    }

    if condition.contains("||") {
        return condition
            .split("||")
            .any(|part| evaluate_condition(part.trim(), context));
    }

    let (var, op, value) = if let Some(pos) = condition.find("==") {
        (condition[..pos].trim(), "==", condition[pos + 2..].trim())
    } else if let Some(pos) = condition.find("!=") {
        (condition[..pos].trim(), "!=", condition[pos + 2..].trim())
    } else if let Some(pos) = condition.find(">=") {
        (condition[..pos].trim(), ">=", condition[pos + 2..].trim())
    } else if let Some(pos) = condition.find("<=") {
        (condition[..pos].trim(), "<=", condition[pos + 2..].trim())
    } else if let Some(pos) = condition.find('>') {
        (condition[..pos].trim(), ">", condition[pos + 1..].trim())
    } else if let Some(pos) = condition.find('<') {
        (condition[..pos].trim(), "<", condition[pos + 1..].trim())
    } else {
        // Simple truthy check
        let val = context.get(condition);
        return match val {
            Some(serde_json::Value::Bool(b)) => *b,
            Some(serde_json::Value::String(s)) => !s.is_empty() && s != "false",
            Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
            Some(serde_json::Value::Null) | None => false,
            _ => true,
        };
    };

    let ctx_val = get_nested_value(context, var);

    let clean_value = if value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    };

    match op {
        "==" => match ctx_val {
            Some(serde_json::Value::String(s)) => s == clean_value,
            Some(serde_json::Value::Number(n)) => {
                if let Ok(num) = clean_value.parse::<f64>() {
                    n.as_f64().unwrap_or(0.0) == num
                } else {
                    false
                }
            }
            Some(serde_json::Value::Bool(b)) => clean_value == b.to_string(),
            _ => false,
        },
        "!=" => match ctx_val {
            Some(serde_json::Value::String(s)) => s != clean_value,
            Some(serde_json::Value::Number(n)) => {
                if let Ok(num) = clean_value.parse::<f64>() {
                    n.as_f64().unwrap_or(0.0) != num
                } else {
                    true
                }
            }
            _ => true,
        },
        ">" => compare_numbers(ctx_val, clean_value, |a, b| a > b),
        "<" => compare_numbers(ctx_val, clean_value, |a, b| a < b),
        ">=" => compare_numbers(ctx_val, clean_value, |a, b| a >= b),
        "<=" => compare_numbers(ctx_val, clean_value, |a, b| a <= b),
        _ => false,
    }
}

fn compare_numbers<F>(ctx_val: Option<&serde_json::Value>, value_str: &str, cmp: F) -> bool
where
    F: Fn(f64, f64) -> bool,
{
    let num = match value_str.parse::<f64>() {
        Ok(n) => n,
        Err(_) => return false,
    };
    match ctx_val {
        Some(serde_json::Value::Number(n)) => cmp(n.as_f64().unwrap_or(0.0), num),
        Some(serde_json::Value::String(s)) => {
            if let Ok(n) = s.parse::<f64>() {
                cmp(n, num)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Load a .cl file from disk
pub fn load_file(path: &str) -> Result<ContextLang, ClError> {
    if let Ok(content) = std::fs::read_to_string(path) {
        return parse(&content);
    }
    let cl_dir =
        std::env::var("CONTEXTLANGUAGE_DIR").unwrap_or_else(|_| "contextlanguage".to_string());
    let cl_path = format!("{}/{}", cl_dir, path);
    if let Ok(content) = std::fs::read_to_string(&cl_path) {
        return parse(&content);
    }
    let contexts_path = format!("contexts/{}", path);
    if let Ok(content) = std::fs::read_to_string(&contexts_path) {
        return parse(&content);
    }
    let data_contexts_path = format!("data/contexts/{}", path);
    let content = std::fs::read_to_string(&data_contexts_path)?;
    parse(&content)
}

/// Evaluate transitions and advance to next state if condition is met.
pub fn advance_state(cl: &ContextLang, context: &serde_json::Value) -> Option<String> {
    let obj = match context.as_object() {
        Some(o) => o,
        None => return None,
    };

    let current_state = obj
        .get("active_state")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    for transition in &cl.transitions {
        if transition.from == current_state && evaluate_condition(&transition.condition, obj) {
            return Some(transition.to.clone());
        }
    }

    None
}

/// Force transition to a specific state.
pub fn transition_to(
    cl: &ContextLang,
    context: &mut serde_json::Value,
    target_state: &str,
) -> bool {
    if let Some(state) = cl.states.get(target_state) {
        for (key, value) in &state.variables {
            set_nested_value(context, key, serde_json::Value::String(value.clone()));
        }
        set_nested_value(
            context,
            "active_state",
            serde_json::Value::String(target_state.to_string()),
        );
        true
    } else {
        false
    }
}

fn set_nested_value(context: &mut serde_json::Value, path: &str, value: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return;
    }

    let mut current = context;
    for part in &parts[..parts.len() - 1] {
        if !current.is_object() {
            *current = serde_json::json!({});
        }
        if !current.as_object().unwrap().contains_key(*part) {
            current
                .as_object_mut()
                .unwrap()
                .insert(part.to_string(), serde_json::json!({}));
        }
        current = current.as_object_mut().unwrap().get_mut(*part).unwrap();
    }

    if let Some(obj) = current.as_object_mut() {
        obj.insert(parts.last().unwrap().to_string(), value);
    }
}

fn get_nested_value<'a>(context: &'a serde_json::Map<String, serde_json::Value>, path: &str) -> Option<&'a serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return None;
    }

    let mut current = context.get(parts[0])?;
    for part in &parts[1..] {
        current = current.get(part)?;
    }
    Some(current)
}

/// Get the next state in the @steps sequence.
pub fn next_step_state(cl: &ContextLang, context: &serde_json::Value) -> Option<String> {
    let obj = match context.as_object() {
        Some(o) => o,
        None => return None,
    };

    let current_state = obj
        .get("active_state")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let pos = cl.steps.iter().position(|s| s == current_state);
    match pos {
        Some(i) if i + 1 < cl.steps.len() => Some(cl.steps[i + 1].clone()),
        _ => None,
    }
}

#[cfg(test)]
mod cl_tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let input = r#"
@name "Test Workflow"
@version "1.0"
@steps [calm, focused]

[state calm]
mode = "chat"
voice = "calm.wav"

[state focused]
mode = "code"
voice = "focused.wav"

[auto]
step == 1 -> use calm
step == 2 -> use focused

[overrides]
if hour < 6 -> style = "whisper"
"#;
        let cl = parse(input).unwrap();
        assert_eq!(cl.name, "Test Workflow");
        assert_eq!(cl.version, "1.0");
        assert_eq!(cl.steps, vec!["calm", "focused"]);
        assert_eq!(cl.states.len(), 2);
        assert_eq!(cl.auto_rules.len(), 2);
        assert_eq!(cl.overrides.len(), 1);
    }

    #[test]
    fn test_parse_state_variables() {
        let input = r#"
[state calm]
mode = "chat"
voice = "calm.wav"
"#;
        let cl = parse(input).unwrap();
        let state = cl.states.get("calm").unwrap();
        assert_eq!(state.variables.get("mode").unwrap(), "chat");
        assert_eq!(state.variables.get("voice").unwrap(), "calm.wav");
    }

    #[test]
    fn test_parse_transitions() {
        let input = r#"
[transitions]
calm -> focused : when step > 1
focused -> calm : when reset
"#;
        let cl = parse(input).unwrap();
        assert_eq!(cl.transitions.len(), 2);
        assert_eq!(cl.transitions[0].from, "calm");
        assert_eq!(cl.transitions[0].to, "focused");
        assert_eq!(cl.transitions[0].condition, "step > 1");
    }

    #[test]
    fn test_parse_auto_rules() {
        let input = r#"
[auto]
step == 1 -> use calm
step == 2 -> use focused
"#;
        let cl = parse(input).unwrap();
        assert_eq!(cl.auto_rules.len(), 2);
        assert_eq!(cl.auto_rules[0].condition, "step == 1");
        assert_eq!(cl.auto_rules[0].target_state, "calm");
    }

    #[test]
    fn test_parse_overrides() {
        let input = r#"
[overrides]
if hour < 6 -> style = "whisper"
if mode == "code" -> voice = "focused.wav"
"#;
        let cl = parse(input).unwrap();
        assert_eq!(cl.overrides.len(), 2);
        assert_eq!(cl.overrides[0].condition, "hour < 6");
        assert_eq!(cl.overrides[0].key, "style");
        assert_eq!(cl.overrides[0].value, "whisper");
    }

    #[test]
    fn test_parse_secret_overrides() {
        let input = r#"
[secrets]
if mode == "code" -> mimo_voice = "coder"
"#;
        let cl = parse(input).unwrap();
        assert_eq!(cl.secret_overrides.len(), 1);
        assert_eq!(cl.secret_overrides[0].key, "mimo_voice");
    }

    #[test]
    fn test_parse_secret_overrides_invalid_field() {
        let input = r#"
[secrets]
if mode == "code" -> invalid_field = "value"
"#;
        let result = parse(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_evaluate_condition_eq() {
        let mut ctx = serde_json::Map::new();
        ctx.insert("step".to_string(), serde_json::json!(1));
        ctx.insert("mode".to_string(), serde_json::json!("chat"));

        assert!(evaluate_condition("step == 1", &ctx));
        assert!(!evaluate_condition("step == 2", &ctx));
        assert!(evaluate_condition("mode == \"chat\"", &ctx));
    }

    #[test]
    fn test_evaluate_condition_neq() {
        let mut ctx = serde_json::Map::new();
        ctx.insert("step".to_string(), serde_json::json!(1));

        assert!(evaluate_condition("step != 2", &ctx));
        assert!(!evaluate_condition("step != 1", &ctx));
    }

    #[test]
    fn test_evaluate_condition_gt_lt() {
        let mut ctx = serde_json::Map::new();
        ctx.insert("step".to_string(), serde_json::json!(5));

        assert!(evaluate_condition("step > 3", &ctx));
        assert!(!evaluate_condition("step > 5", &ctx));
        assert!(evaluate_condition("step < 10", &ctx));
        assert!(evaluate_condition("step >= 5", &ctx));
        assert!(evaluate_condition("step <= 5", &ctx));
    }

    #[test]
    fn test_evaluate_condition_compound() {
        let mut ctx = serde_json::Map::new();
        ctx.insert("step".to_string(), serde_json::json!(1));
        ctx.insert("mode".to_string(), serde_json::json!("chat"));

        assert!(evaluate_condition("step == 1 && mode == \"chat\"", &ctx));
        assert!(!evaluate_condition("step == 1 && mode == \"code\"", &ctx));
        assert!(evaluate_condition("step == 1 || mode == \"code\"", &ctx));
    }

    #[test]
    fn test_evaluate_condition_truthy() {
        let mut ctx = serde_json::Map::new();
        ctx.insert("flag".to_string(), serde_json::json!(true));
        ctx.insert("empty".to_string(), serde_json::json!(""));

        assert!(evaluate_condition("flag", &ctx));
        assert!(!evaluate_condition("empty", &ctx));
        assert!(!evaluate_condition("nonexistent", &ctx));
    }

    #[test]
    fn test_apply_to_context() {
        let input = r#"
[state calm]
mode = "chat"
voice = "calm.wav"

[state focused]
mode = "code"
voice = "focused.wav"

[auto]
step == 1 -> use calm
step == 2 -> use focused
"#;
        let cl = parse(input).unwrap();
        let mut ctx = serde_json::json!({"step": 1});
        apply_to_context(&cl, &mut ctx);

        assert_eq!(ctx["mode"], "chat");
        assert_eq!(ctx["voice"], "calm.wav");
        assert_eq!(ctx["active_state"], "calm");
    }

    #[test]
    fn test_apply_overrides() {
        let input = r#"
[state calm]
mode = "chat"

[overrides]
if hour < 6 -> mode = "whisper"
"#;
        let cl = parse(input).unwrap();
        let mut ctx = serde_json::json!({"step": 1, "hour": 3});
        apply_to_context(&cl, &mut ctx);

        assert_eq!(ctx["mode"], "whisper");
    }

    #[test]
    fn test_advance_state() {
        let input = r#"
@steps [calm, focused]

[state calm]
mode = "chat"

[state focused]
mode = "code"

[transitions]
calm -> focused : when step > 1
"#;
        let cl = parse(input).unwrap();
        let ctx = serde_json::json!({"active_state": "calm", "step": 2});

        let new_state = advance_state(&cl, &ctx);
        assert_eq!(new_state, Some("focused".to_string()));
    }

    #[test]
    fn test_advance_state_no_transition() {
        let input = r#"
[state calm]
mode = "chat"

[transitions]
calm -> focused : when step > 1
"#;
        let cl = parse(input).unwrap();
        let ctx = serde_json::json!({"active_state": "calm", "step": 1});

        let new_state = advance_state(&cl, &ctx);
        assert!(new_state.is_none());
    }

    #[test]
    fn test_transition_to() {
        let input = r#"
[state calm]
mode = "chat"

[state focused]
mode = "code"
"#;
        let cl = parse(input).unwrap();
        let mut ctx = serde_json::json!({"active_state": "calm"});

        let success = transition_to(&cl, &mut ctx, "focused");
        assert!(success);
        assert_eq!(ctx["mode"], "code");
        assert_eq!(ctx["active_state"], "focused");
    }

    #[test]
    fn test_transition_to_nonexistent() {
        let input = r#"
[state calm]
mode = "chat"
"#;
        let cl = parse(input).unwrap();
        let mut ctx = serde_json::json!({"active_state": "calm"});

        let success = transition_to(&cl, &mut ctx, "nonexistent");
        assert!(!success);
    }

    #[test]
    fn test_next_step_state() {
        let input = r#"
@steps [calm, focused, review]

[state calm]
[state focused]
[state review]
"#;
        let cl = parse(input).unwrap();
        let ctx = serde_json::json!({"active_state": "calm"});

        let next = next_step_state(&cl, &ctx);
        assert_eq!(next, Some("focused".to_string()));
    }

    #[test]
    fn test_next_step_state_last() {
        let input = r#"
@steps [calm, focused]

[state calm]
[state focused]
"#;
        let cl = parse(input).unwrap();
        let ctx = serde_json::json!({"active_state": "focused"});

        let next = next_step_state(&cl, &ctx);
        assert!(next.is_none());
    }

    #[test]
    fn test_parse_empty() {
        let cl = parse("").unwrap();
        assert!(cl.states.is_empty());
    }

    #[test]
    fn test_parse_unknown_section() {
        let input = "[unknown]";
        let result = parse(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_full_workflow() {
        let input = r#"
@name "default"
@version "1.0"
@steps [understand, plan, code, review]

[state understand]
mode = "agent"
task = "understand the task"

[state plan]
mode = "agent"
task = "create a plan"

[state code]
mode = "code"
task = "write code"

[state review]
mode = "agent"
task = "review code"

[transitions]
understand -> plan : when understood
plan -> code : when approved
code -> review : when done

[auto]
turn > 10 -> use plan
"#;
        let cl = parse(input).unwrap();
        assert_eq!(cl.name, "default");
        assert_eq!(cl.steps.len(), 4);
        assert_eq!(cl.states.len(), 4);
        assert_eq!(cl.transitions.len(), 3);
        assert_eq!(cl.auto_rules.len(), 1);
    }
}
