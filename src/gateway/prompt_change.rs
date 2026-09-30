//! Detects system-prompt changes (template updates via SM files, manual
//! settings.system_template switches, memory changes) and injects a system
//! notice into the conversation so the LLM knows its instructions changed.
//!
//! Without this, the model keeps following the previous prompt pattern from
//! conversation history until the user manually points out the update.
//! The notice includes a diff summary so the model knows WHICH instructions
//! changed, not just that something changed.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn hash_prompt(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// Hash a rendered system prompt (public helper for callers that need the
/// hash before moving the prompt into a message).
pub fn hash_system_prompt(text: &str) -> u64 {
    hash_prompt(text)
}

/// Per-user registry of the last known system prompt (hash + text) so a
/// change can be diffed. Entries are overwritten on every turn.
#[derive(Clone)]
struct PromptBaseline {
    hash: u64,
    text: String,
}
static LAST_PROMPT: once_cell::sync::Lazy<dashmap::DashMap<String, PromptBaseline>> =
    once_cell::sync::Lazy::new(dashmap::DashMap::new);

/// Unified-diff-style summary of two prompts: lines removed (-) and added (+).
/// Unchanged context lines are omitted to keep the notice compact.
fn prompt_diff_summary(old_text: &str, new_text: &str) -> String {
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();
    let new_set: std::collections::HashSet<&str> = new_lines.iter().copied().collect();
    let old_set: std::collections::HashSet<&str> = old_lines.iter().copied().collect();

    let mut removed: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    for l in &old_lines {
        if !new_set.contains(l) {
            removed.push(l.to_string());
        }
    }
    for l in &new_lines {
        if !old_set.contains(l) {
            added.push(l.to_string());
        }
    }

    let fmt = |lines: &[String], prefix: &str| -> Vec<String> {
        lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .take(12)
            .map(|l| {
                let mut s = format!("{} {}", prefix, l.trim());
                if s.chars().count() > 160 {
                    s = format!("{}…", s.chars().take(160).collect::<String>());
                }
                s
            })
            .collect()
    };

    let mut out = Vec::new();
    let removed_fmt = fmt(&removed, "-");
    let added_fmt = fmt(&added, "+");
    if !removed_fmt.is_empty() {
        out.push("Removed:".to_string());
        out.extend(removed_fmt);
    }
    if !added_fmt.is_empty() {
        out.push("Added:".to_string());
        out.extend(added_fmt);
    }
    if out.is_empty() {
        "(no line-level differences; whitespace-only change)".to_string()
    } else {
        out.join("\n")
    }
}

/// Compare the freshly rendered system prompt against the last known one.
/// Returns Some(notice_text) when the prompt changed and the difference should
/// be announced to the model. The first call for a user returns None (no
/// baseline to compare against). Preferred entry point: pass the full text.
pub fn take_prompt_change_notice(user_id: &str, rendered_system_prompt: &str) -> Option<String> {
    take_prompt_change_notice_text(user_id, rendered_system_prompt)
}

/// Text-based variant: stores the baseline and diffs on change.
pub fn take_prompt_change_notice_text(
    user_id: &str,
    rendered_system_prompt: &str,
) -> Option<String> {
    let new_hash = hash_prompt(rendered_system_prompt);
    let mut entry = LAST_PROMPT
        .entry(user_id.to_string())
        .or_insert_with(|| PromptBaseline {
            hash: new_hash,
            text: rendered_system_prompt.to_string(),
        });
    if entry.hash == new_hash {
        return None;
    }
    let diff = prompt_diff_summary(&entry.text, rendered_system_prompt);
    *entry = PromptBaseline {
        hash: new_hash,
        text: rendered_system_prompt.to_string(),
    };
    Some(format!(
        "[SYSTEM PROMPT UPDATED] Your system instructions have changed since your last reply \
         (template update or profile switch). Re-read the new system prompt above and follow \
         it from now on; previous patterns from earlier conversation may be outdated. \
         Changes:\n{}",
        diff
    ))
}

/// Hash-based variant for callers that only have the hash (no text available).
/// Falls back to the generic notice without a diff.
pub fn take_prompt_change_notice_hash(user_id: &str, hash: u64) -> Option<String> {
    let mut entry = LAST_PROMPT
        .entry(user_id.to_string())
        .or_insert_with(|| PromptBaseline {
            hash,
            text: String::new(),
        });
    if entry.hash == hash {
        return None;
    }
    *entry = PromptBaseline {
        hash,
        text: String::new(),
    };
    Some(
        "[SYSTEM PROMPT UPDATED] Your system instructions have changed since your last reply \
         (template update or profile switch). Re-read the new system prompt above and follow \
         it from now on; previous patterns from earlier conversation may be outdated."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_call_has_no_notice() {
        let id = "u-first-notice";
        assert!(take_prompt_change_notice(id, "prompt A").is_none());
    }

    #[test]
    fn change_returns_notice_once() {
        let id = "u-change-notice";
        take_prompt_change_notice(id, "prompt A");
        let notice = take_prompt_change_notice(id, "prompt B");
        assert!(notice.is_some());
        assert!(take_prompt_change_notice(id, "prompt B").is_none());
    }

    #[test]
    fn same_prompt_no_notice() {
        let id = "u-same-notice";
        take_prompt_change_notice(id, "prompt X");
        assert!(take_prompt_change_notice(id, "prompt X").is_none());
    }

    #[test]
    fn notice_contains_diff_lines() {
        let id = "u-diff-notice";
        let old = "line one\nline two\nline three";
        let new = "line one\nline TWO changed\nline three\nline four added";
        take_prompt_change_notice(id, old);
        let notice = take_prompt_change_notice(id, new).expect("notice on change");
        assert!(notice.contains("- line two"), "removed line in diff: {}", notice);
        assert!(notice.contains("+ line TWO changed"), "added line in diff: {}", notice);
        assert!(notice.contains("+ line four added"), "added line in diff: {}", notice);
        assert!(!notice.contains("- line one"), "unchanged line must not appear");
        // Second call with same prompt: no notice.
        assert!(take_prompt_change_notice(id, new).is_none());
    }

    #[test]
    fn diff_summary_formats() {
        let d = prompt_diff_summary("a\nb", "a\nc\nd");
        assert!(d.contains("- b"));
        assert!(d.contains("+ c"));
        assert!(d.contains("+ d"));
        // Blank-line-only change: all diff lines are empty and filtered out,
        // so the summary reports a whitespace-only change.
        let ws = prompt_diff_summary("a\n\nb", "a\n\n\nb");
        assert!(ws.contains("whitespace-only"));
    }
}