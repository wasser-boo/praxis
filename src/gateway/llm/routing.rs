use super::{
    error::{CallFailure, ErrorKind, ProviderError},
    provider::{ChatRequest, ChatResponse, ContentPart, LLMProvider},
    resilience::{self, interruptible, ProviderGate, ResilienceConfig},
    LLMRouter,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

impl LLMRouter {
    pub(crate) fn with_providers(
        providers: Vec<Box<dyn LLMProvider>>,
        default_provider: String,
        fallback_providers: Vec<String>,
        policy: ResilienceConfig,
    ) -> Self {
        let gates = providers
            .iter()
            .map(|p| (p.name().to_string(), Arc::new(ProviderGate::new(&policy))))
            .collect::<HashMap<_, _>>();
        Self {
            providers,
            default_provider,
            fallback_providers,
            policy,
            gates,
        }
    }

    pub fn validate_configuration(&self) -> anyhow::Result<()> {
        self.policy.validate()?;
        for name in std::iter::once(&self.default_provider).chain(self.fallback_providers.iter()) {
            self.find_provider(name)?;
        }
        Ok(())
    }

    /// Configured first-attempt bound for application task requests, including
    /// every tool continuation. Explicit low-level requests/summaries keep their
    /// own max_tokens; retries still obey the separate growth ceiling.
    pub fn task_output_tokens(&self) -> u32 {
        // validate_configuration/route reject invalid settings before inference.
        self.policy.max_output_tokens.min(u64::from(u32::MAX)) as u32
    }

    pub fn history_image_messages(&self) -> usize {
        self.policy.history_image_messages.min(2) as usize
    }

    fn find_provider(&self, name: &str) -> Result<&dyn LLMProvider, anyhow::Error> {
        self.providers
            .iter()
            .find(|p| p.name() == name)
            .map(|p| p.as_ref())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "LLM '{}': {}. Registered: {}",
                    label(name),
                    ProviderError::new(ErrorKind::Configuration),
                    self.provider_names()
                        .iter()
                        .map(|s| label(s))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
    }

    pub async fn chat(
        &self,
        request: ChatRequest,
        provider: Option<&str>,
    ) -> anyhow::Result<ChatResponse> {
        self.chat_controlled(request, provider, None, &CancellationToken::new())
            .await
    }

    pub async fn streaming_chat(
        &self,
        request: ChatRequest,
        provider: Option<&str>,
        user_id: &str,
    ) -> anyhow::Result<ChatResponse> {
        let cancel = crate::gateway::task_control::cancellation(user_id).unwrap_or_default();
        self.chat_controlled(request, provider, Some(user_id), &cancel)
            .await
    }

    /// One logical LLM call. This is deliberately BELOW all tool execution:
    /// retries can never replay a terminal command, file write or upload.
    pub async fn chat_controlled(
        &self,
        request: ChatRequest,
        provider: Option<&str>,
        user: Option<&str>,
        cancel: &CancellationToken,
    ) -> anyhow::Result<ChatResponse> {
        let result = self.route(request, provider, user, cancel).await;
        if let Err(error) = &result {
            if let Some(user) = user {
                crate::dashboard::stream::send(user, "stream_abort", "{}");
            }
            resilience::progress(user, &error.to_string());
        }
        result
    }

    async fn route(
        &self,
        mut request: ChatRequest,
        provider: Option<&str>,
        user: Option<&str>,
        cancel: &CancellationToken,
    ) -> anyhow::Result<ChatResponse> {
        self.policy.validate()?;
        let started = Instant::now();
        let deadline = started + Duration::from_millis(self.policy.total_timeout_ms);
        let base_provider = provider.unwrap_or(&self.default_provider);
        let has_images = request.messages.iter().any(|m| {
            m.content_parts.as_ref().is_some_and(|parts| {
                parts
                    .iter()
                    .any(|p| matches!(p, ContentPart::ImageUrl { .. }))
            })
        });
        let primary = if has_images {
            let selected = request.vision_provider.as_deref().unwrap_or(base_provider);
            if selected != base_provider {
                request.model = None;
            }
            if let Some(model) = &request.vision_model {
                request.model = Some(model.clone());
            }
            selected.to_string()
        } else {
            base_provider.to_string()
        };
        let mut candidates = vec![primary];
        for name in &self.fallback_providers {
            if !candidates.contains(name) {
                candidates.push(name.clone());
            }
        }
        // Missing/typo'd providers are configuration errors, not a reason to
        // silently transmit a user's conversation to a different account.
        for name in &candidates {
            self.find_provider(name)?;
        }

        let mut attempts = 0usize;
        let mut failures = Vec::new();
        let mut terminal = ProviderError::new(ErrorKind::Unavailable);
        let max_attempts = self.policy.max_attempts as usize;
        for (index, name) in candidates.iter().enumerate() {
            if attempts >= max_attempts {
                break;
            }
            let p = self.find_provider(name)?;
            let gate = &self.gates[name];
            let _admission = match gate.admit() {
                Ok(permit) => permit,
                Err(error) => {
                    terminal = error;
                    break;
                }
            };
            let mut req = request.clone();
            if index > 0 {
                // Use the explicitly configured fallback's own default model.
                req.model = None;
                req.vision_provider = None;
                req.vision_model = None;
                resilience::progress(
                    user,
                    &format!("LLM switching to configured fallback {}.", label(name)),
                );
            }
            // Reserve one attempt for each remaining explicit fallback. The
            // attempt cap and time budget apply to the WHOLE route, not per hop.
            let allowance =
                max_attempts.saturating_sub((candidates.len() - index - 1).min(max_attempts - 1));
            while attempts < allowance {
                // Output-limit recovery can enlarge the next request. Reserve
                // its full allowance through the same gate before sending it.
                let tokens = resilience::estimated_tokens(&req);
                let permit = match interruptible(deadline, cancel, gate.acquire(tokens, user)).await
                {
                    Ok(Ok(permit)) => permit,
                    Ok(Err(error)) | Err(error) => {
                        return Err(CallFailure {
                            attempts,
                            failures,
                            terminal: error,
                        }
                        .into());
                    }
                };
                attempts += 1;
                let attempt_start = Instant::now();
                let attempt_deadline = deadline
                    .min(attempt_start + Duration::from_millis(self.policy.request_timeout_ms));
                let mut stream = StreamAttempt::new(user);
                let on_token = |token: String| {
                    if token.is_empty() {
                        return;
                    }
                    stream.emitted.store(true, Ordering::Relaxed);
                    if let Some(user) = user {
                        let mut buf = [0u8; 4];
                        for ch in token.chars() {
                            crate::dashboard::stream::send(user, "char", ch.encode_utf8(&mut buf));
                        }
                    }
                };
                tracing::info!(provider = %label(name), attempt = attempts, "LLM attempt started");
                let call = async {
                    if user.is_some() {
                        p.chat_stream(req.clone(), &on_token).await
                    } else {
                        p.chat(req.clone()).await
                    }
                };
                let result = match interruptible(attempt_deadline, cancel, call).await {
                    Ok(Ok(response)) => validate_response(response),
                    Ok(Err(error)) => Err(ProviderError::from_anyhow(&error)),
                    Err(mut error) => {
                        if error.kind == ErrorKind::Deadline && attempt_deadline < deadline {
                            error.kind = ErrorKind::Timeout;
                        }
                        Err(error)
                    }
                };
                match result {
                    Ok(response) => {
                        if let Some(usage) = &response.usage {
                            let actual = u64::from(usage.total_tokens).max(
                                u64::from(usage.prompt_tokens) + u64::from(usage.completion_tokens),
                            );
                            if actual > 0 {
                                gate.reconcile(permit.reservation, actual);
                            }
                        }
                        stream.committed = true;
                        if let (Some(user), Some(text)) =
                            (user, response.content.as_deref().filter(|s| !s.trim().is_empty()))
                        {
                            crate::dashboard::stream::send(user, "assistant", text);
                        }
                        tracing::info!(provider = %label(name), attempt = attempts, elapsed_ms = attempt_start.elapsed().as_millis() as u64, content_bytes = response.content.as_ref().map_or(0, String::len), tool_call_count = response.tool_calls.as_ref().map_or(0, Vec::len), "LLM attempt succeeded");
                        return Ok(response);
                    }
                    Err(mut error) => {
                        let transient_failure = error.kind.retryable();
                        // Once text has reached the UI, a new generation cannot
                        // safely be appended or silently substituted. Discard
                        // the unfinished bubble and leave recovery to the user.
                        if stream.emitted.load(Ordering::Relaxed)
                            && error.kind != ErrorKind::Cancelled
                        {
                            error.kind = ErrorKind::PartialStream;
                        }
                        tracing::warn!(provider = %label(name), attempt = attempts, elapsed_ms = attempt_start.elapsed().as_millis() as u64, error = %error, "LLM attempt failed");
                        failures.push((label(name), error.clone()));
                        // A known output cutoff is recoverable only before any
                        // visible text, with a strictly larger bounded allowance.
                        // This remains below tool execution and inside the same
                        // global attempt/time/rate limits; no hidden adapter retry.
                        let expanded_output = if error.kind == ErrorKind::OutputLimit
                            && attempts < allowance
                        {
                            self.policy.next_output_limit(req.max_tokens)
                        } else {
                            None
                        };
                        if let Some(tokens) = expanded_output {
                            req.max_tokens = Some(tokens);
                        }
                        let retryable = error.kind.retryable() || expanded_output.is_some();
                        terminal = error.clone();
                        if transient_failure || expanded_output.is_some() {
                            let delay = self
                                .policy
                                .backoff(attempts)
                                .max(error.retry_after.unwrap_or_default());
                            // Even the LAST 429 updates shared cooldown so a new
                            // user task cannot immediately hammer this account.
                            gate.defer(delay);
                            if retryable && attempts < allowance {
                                let output_notice = expanded_output
                                    .map(|tokens| format!(" with {tokens} output tokens"))
                                    .unwrap_or_default();
                                resilience::progress(
                                    user,
                                    &format!(
                                        "LLM {}: retry {}/{}{output_notice} in at least {:.1}s ({error}).",
                                        label(name),
                                        attempts + 1,
                                        max_attempts,
                                        delay.as_secs_f64()
                                    ),
                                );
                            }
                        }
                        drop(stream);
                        drop(permit); // no active permit held during retry sleep
                        if !retryable {
                            return Err(CallFailure {
                                attempts,
                                failures,
                                terminal,
                            }
                            .into());
                        }
                    }
                }
            }
        }
        Err(CallFailure {
            attempts,
            failures,
            terminal,
        }
        .into())
    }

    pub async fn health_check(&self) -> bool {
        if self.validate_configuration().is_err() {
            return false;
        }
        let Ok(provider) = self.find_provider(&self.default_provider) else {
            return false;
        };
        tokio::time::timeout(Duration::from_secs(10), provider.health_check())
            .await
            .unwrap_or(false)
    }
}

fn label(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || "-_./".contains(*ch))
        .take(80)
        .collect()
}

fn validate_response(mut response: ChatResponse) -> Result<ChatResponse, ProviderError> {
    if response
        .tool_calls
        .as_ref()
        .is_some_and(|calls| calls.is_empty())
    {
        response.tool_calls = None;
    }
    let tools = response.tool_calls.as_deref().unwrap_or_default();
    if tools.is_empty()
        && response
            .content
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    {
        return Err(ProviderError::new(ErrorKind::InvalidResponse));
    }
    let mut ids = std::collections::HashSet::new();
    for call in tools {
        if call.id.is_empty()
            || !ids.insert(&call.id)
            || call.function.name.is_empty()
            || !serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                .is_ok_and(|args| args.is_object())
        {
            return Err(ProviderError::new(ErrorKind::InvalidResponse));
        }
    }
    Ok(response)
}

struct StreamAttempt<'a> {
    user: Option<&'a str>,
    emitted: AtomicBool,
    committed: bool,
}
impl<'a> StreamAttempt<'a> {
    fn new(user: Option<&'a str>) -> Self {
        if let Some(user) = user {
            crate::dashboard::stream::send(user, "typing", "true");
        }
        Self {
            user,
            emitted: AtomicBool::new(false),
            committed: false,
        }
    }
}
impl Drop for StreamAttempt<'_> {
    fn drop(&mut self) {
        if let Some(user) = self.user {
            if !self.committed && self.emitted.load(Ordering::Relaxed) {
                crate::dashboard::stream::send(user, "stream_abort", "{}");
            }
            crate::dashboard::stream::send(user, "typing", "false");
        }
    }
}
