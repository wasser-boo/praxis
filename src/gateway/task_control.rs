//! Per-user task ownership and cancellation, shared by HTTP, WS and agent paths.
//! A cancelled task retains ownership until any already-running tool has saved
//! its result. Starting a second task in that interval would risk duplicate effects.
use dashmap::{mapref::entry::Entry, DashMap};
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

static TASKS: Lazy<DashMap<String, Arc<CancellationToken>>> = Lazy::new(DashMap::new);

pub struct TaskGuard {
    user: String,
    token: Arc<CancellationToken>,
}

pub fn begin(user: &str) -> anyhow::Result<TaskGuard> {
    match TASKS.entry(user.to_string()) {
        Entry::Occupied(_) => anyhow::bail!(
            "A task is already running for this user; wait, send agent input, or stop it first"
        ),
        Entry::Vacant(entry) => {
            let token = Arc::new(CancellationToken::new());
            entry.insert(token.clone());
            Ok(TaskGuard {
                user: user.into(),
                token,
            })
        }
    }
}
pub fn cancellation(user: &str) -> Option<CancellationToken> {
    TASKS.get(user).map(|entry| entry.value().as_ref().clone())
}
pub fn cancel(user: &str) {
    if let Some(token) = cancellation(user) {
        token.cancel();
    }
}
impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.token.cancel();
        TASKS.remove_if(&self.user, |_, token| Arc::ptr_eq(token, &self.token));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resilience_cancel_keeps_ownership_until_result_is_saved() {
        let user = "resilience-task-ownership";
        let guard = begin(user).unwrap();
        let token = cancellation(user).unwrap();
        assert!(begin(user).is_err());
        cancel(user);
        assert!(token.is_cancelled());
        assert!(begin(user).is_err());
        drop(guard);
        assert!(cancellation(user).is_none());
        let next = begin(user).unwrap();
        assert!(!cancellation(user).unwrap().is_cancelled());
        drop(next);
    }
}
