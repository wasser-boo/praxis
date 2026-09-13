//! Detects system-prompt changes (template updates via SM files, manual
//! settings.system_template switches, memory changes) and injects a system
//! notice into the conversation so the LLM knows its instructions changed.
//!
//! Without this, the model keeps following the previous prompt pattern from
//! conversation history until the user manually points out the update.

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

/// In-memory registry of the last known system-prompt hash per user.
static LAST_PROMPT_HASH: once_cell::sync::Lazy<dashmap::DashMap<String, u64>> =
    once_cell::sync::Lazy::new(dashmap::DashMap::new);

/// Compare the freshly rendered system prompt against the last known one.
/// Returns Some(notice_text) when the prompt changed and the difference should
/// be announced to the model. The first call for a user returns None (no
/// baseline to compare against).
pub fn take_prompt_change_notice(user_id: &str, rendered_system_prompt: &str) -> Option<String> {
    take_prompt_change_notice_hash(user_id, hash_prompt(rendered_system_prompt))
}

/// Hash-based variant for callers that computed the hash before moving the
/// prompt string into a message.
pub fn take_prompt_change_notice_hash(user_id: &str, hash: u64) -> Option<String> {
    let mut entry = LAST_PROMPT_HASH.entry(user_id.to_string()).or_insert(hash);
    let prev = *entry;
    if prev == hash {
        return None;
    }
    *entry = hash;
    Some(format!(
        "[SYSTEM PROMPT UPDATED] Your system instructions have changed since your last reply \
         (template update or profile switch). Re-read the new system prompt above and follow \
         it from now on; previous patterns from earlier conversation may be outdated."
    ))
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
}