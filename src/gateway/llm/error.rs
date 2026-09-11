//! Safe, machine-readable errors. Never retain provider bodies, prompts, URLs or keys.
use std::{fmt, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Configuration,
    Authentication,
    QuotaExhausted,
    InvalidRequest,
    InvalidResponse,
    OutputLimit,
    RateLimited,
    Unavailable,
    Transport,
    Timeout,
    Interrupted,
    PartialStream,
    Cancelled,
    QueueFull,
    Deadline,
    TokenBudget,
}

impl ErrorKind {
    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited
                | Self::Unavailable
                | Self::Transport
                | Self::Timeout
                | Self::Interrupted
        )
    }
}
impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "provider not configured (check USE_PROVIDER, settings.provider and credentials; restart after configuration changes)",
            Self::Authentication => "authentication/permission rejected",
            Self::QuotaExhausted => "quota or credit balance exhausted",
            Self::InvalidRequest => "invalid request or unsupported model/parameters (check model, tools and context size)",
            Self::InvalidResponse => "invalid or empty provider response",
            Self::OutputLimit => "model output token limit reached (request less output or increase LLM_RETRY_MAX_OUTPUT_TOKENS)",
            Self::RateLimited => "rate limited",
            Self::Unavailable => "provider temporarily unavailable",
            Self::Transport => "transport failure",
            Self::Timeout => "request timed out",
            Self::Interrupted => "response stream interrupted",
            Self::PartialStream => "partial stream interrupted; not replayed and no tools from this response executed",
            Self::Cancelled => "cancelled",
            Self::QueueFull => "LLM queue full; try again later",
            Self::Deadline => "LLM time budget exhausted (including queue, requests and retry waits)",
            Self::TokenBudget => "request exceeds configured LLM_TOKENS_PER_MINUTE; reduce context/output or increase the limit",
        })
    }
}

#[derive(Debug, Clone)]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub status: Option<u16>,
    pub retry_after: Option<Duration>,
    pub request_id: Option<String>,
    /// A controlled diagnostic, NOT an arbitrary server/reqwest error message.
    pub cause: Option<&'static str>,
}
impl ProviderError {
    pub fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            status: None,
            retry_after: None,
            request_id: None,
            cause: None,
        }
    }
    pub fn from_reqwest(error: reqwest::Error) -> Self {
        let error = error.without_url();
        // Inspect the chain to distinguish real I/O causes without exposing its
        // potentially credential-bearing URLs. Do not log `error` itself.
        let mut cause = None;
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        while let Some(e) = current {
            let text = e.to_string().to_ascii_lowercase();
            for (needle, diagnostic) in [
                (
                    "invalid peer certificate",
                    "TLS certificate validation failed",
                ),
                (
                    "certificate verify failed",
                    "TLS certificate validation failed",
                ),
                ("unknownissuer", "TLS certificate validation failed"),
                ("dns", "DNS lookup failed"),
                ("connection refused", "connection refused"),
                ("connection reset", "connection reset"),
                ("broken pipe", "broken pipe"),
                ("unexpected eof", "unexpected EOF"),
                ("connection closed", "connection closed"),
                ("incomplete message", "incomplete HTTP message"),
                ("timed out", "I/O timeout"),
            ] {
                if text.contains(needle) {
                    cause = Some(diagnostic);
                }
            }
            current = e.source();
        }
        let kind = if error.is_builder() || cause == Some("TLS certificate validation failed") {
            ErrorKind::Configuration
        } else if error.is_timeout() {
            ErrorKind::Timeout
        } else if error.is_decode() {
            ErrorKind::InvalidResponse
        } else {
            ErrorKind::Transport
        };
        Self {
            cause: cause.or_else(|| {
                error
                    .is_connect()
                    .then_some("connection establishment failed")
            }),
            ..Self::new(kind)
        }
    }
    pub fn from_anyhow(error: &anyhow::Error) -> Self {
        error
            .downcast_ref::<Self>()
            .cloned()
            .unwrap_or_else(|| Self::new(ErrorKind::InvalidResponse))
    }
}
impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(status) = self.status {
            write!(f, " [HTTP {status}]")?;
        }
        if let Some(cause) = self.cause {
            write!(f, ": {cause}")?;
        }
        if let Some(delay) = self.retry_after {
            write!(f, " (Retry-After: {}s)", delay.as_secs())?;
        }
        if let Some(id) = &self.request_id {
            write!(f, " (request_id={id})")?;
        }
        Ok(())
    }
}
impl std::error::Error for ProviderError {}

#[derive(Debug)]
pub struct CallFailure {
    pub attempts: usize,
    pub failures: Vec<(String, ProviderError)>,
    pub terminal: ProviderError,
}
impl fmt::Display for CallFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LLM request failed after {} attempt(s): {}",
            self.attempts, self.terminal
        )?;
        for (provider, error) in &self.failures {
            write!(f, "; {provider}: {error}")?;
        }
        Ok(())
    }
}
impl std::error::Error for CallFailure {}
