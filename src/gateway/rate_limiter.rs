use governor::{clock::DefaultClock, state::keyed::DefaultKeyedStateStore, Quota, RateLimiter};
use std::num::NonZeroU32;

pub struct UserRateLimiter {
    limiter: RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>,
}

impl UserRateLimiter {
    pub fn new(requests_per_minute: u32) -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(requests_per_minute).unwrap());
        Self {
            limiter: RateLimiter::keyed(quota),
        }
    }

    pub fn check(&self, user_id: &str) -> Result<(), RateLimitError> {
        self.limiter
            .check_key(&user_id.to_string())
            .map_err(|_| RateLimitError::TooManyRequests)
    }
}

#[derive(Debug)]
pub enum RateLimitError {
    TooManyRequests,
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rate limit exceeded")
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_rate_limiter_allows() {
        let limiter = UserRateLimiter::new(10);
        assert!(limiter.check("user1").is_ok());
    }

    #[test]
    fn test_rate_limiter_blocks() {
        let limiter = UserRateLimiter::new(1);
        assert!(limiter.check("user1").is_ok());
        assert!(limiter.check("user1").is_err());
    }
}
