//! Opt-in external feedback and a per-user/session sliding-window limit.
//! Web progress remains visible even when external speech/Discord is disabled.
use crate::db::contexts::ContextSettings;
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct FeedbackLimiter(Mutex<HashMap<String, VecDeque<Instant>>>);

impl FeedbackLimiter {
    pub fn allow(&self, key: &str, settings: &ContextSettings, now: Instant) -> bool {
        if !settings.feedback_enabled
            || !(1..=1000).contains(&settings.feedback_max_per_5min)
            || !(1..=86400).contains(&settings.feedback_window_secs)
        {
            return false;
        }
        let window = Duration::from_secs(settings.feedback_window_secs as u64);
        let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // Bound retained state, even as sessions/users come and go.
        entries.retain(|_, hits| {
            hits.back()
                .is_some_and(|at| now.saturating_duration_since(*at) < Duration::from_secs(86400))
        });
        if !entries.contains_key(key) && entries.len() >= 10_000 {
            return false;
        }
        let hits = entries.entry(key.into()).or_default();
        while hits
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= window)
        {
            hits.pop_front();
        }
        if hits.len() >= settings.feedback_max_per_5min as usize {
            return false;
        }
        hits.push_back(now);
        true
    }
}

pub(super) static LIMITER: once_cell::sync::Lazy<FeedbackLimiter> =
    once_cell::sync::Lazy::new(FeedbackLimiter::default);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedback_requires_opt_in_and_enforces_per_session_window() {
        let limiter = FeedbackLimiter::default();
        let now = Instant::now();
        let mut settings = ContextSettings::default();
        settings.feedback_mode = vec!["tts".into()];
        assert!(!limiter.allow("alice:one", &settings, now));
        settings.feedback_enabled = true;
        settings.feedback_max_per_5min = 2;
        settings.feedback_window_secs = 10;
        assert!(limiter.allow("alice:one", &settings, now));
        assert!(limiter.allow("alice:one", &settings, now));
        assert!(!limiter.allow("alice:one", &settings, now));
        assert!(limiter.allow("alice:two", &settings, now));
        assert!(limiter.allow("bob:one", &settings, now));
        assert!(limiter.allow("alice:one", &settings, now + Duration::from_secs(10)));
        settings.feedback_max_per_5min = 0;
        assert!(!limiter.allow("alice:one", &settings, now));
        settings.feedback_max_per_5min = 2;
        settings.feedback_window_secs = -1;
        assert!(!limiter.allow("alice:one", &settings, now));
    }
}
