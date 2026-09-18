//! Real local HTTP contracts for the actual adapters, not just mock traits.
use super::{provider::*, resilience::ResilienceConfig, LLMRouter};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request() -> ChatRequest {
    ChatRequest {
        messages: vec![],
        tools: None,
        temperature: None,
        max_tokens: Some(10),
        model: None,
        vision_provider: None,
        vision_model: None,
        thinking: None,
    }
}
fn policy() -> ResilienceConfig {
    ResilienceConfig {
        max_attempts: 2,
        initial_backoff_ms: 1,
        max_backoff_ms: 1,
        request_timeout_ms: 2000,
        total_timeout_ms: 5000,
        ..Default::default()
    }
}
fn adapter(name: &str, base: String) -> (Box<dyn LLMProvider>, &'static str, serde_json::Value) {
    let key = "local-test-only".to_string();
    let model = "test-model".to_string();
    let openai =
        serde_json::json!({"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]});
    let anthropic =
        serde_json::json!({"content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn"});
    match name {
        "openai" => (
            Box::new(super::openai::OpenAIProvider::new(key, model, base)),
            "/chat/completions",
            openai,
        ),
        "openrouter" => (
            Box::new(super::openrouter::OpenRouterProvider::new(key, model, base)),
            "/chat/completions",
            openai,
        ),
        "llamacpp" => (
            Box::new(super::llamacpp::LlamaCppProvider::new(
                Some(key),
                model,
                base,
            )),
            "/v1/chat/completions",
            openai,
        ),
        "anthropic" => (
            Box::new(super::anthropic::AnthropicProvider::new(key, model, base)),
            "/v1/messages",
            anthropic,
        ),
        "ollama" => (
            Box::new(super::ollama::OllamaProvider::new(base, model, Some(key))),
            "/api/chat",
            serde_json::json!({"message":{"content":"ok"},"done":true,"prompt_eval_count":5,"eval_count":2}),
        ),
        "minimax" => (
            Box::new(super::minimax::MiniMaxProvider::new(
                key,
                model,
                base,
                crate::config::ApiMode::OpenAI,
            )),
            "/text/chatcompletion_v2",
            openai,
        ),
        "mimo" => (
            Box::new(super::mimo::MiMoProvider::new(
                key,
                model,
                base,
                crate::config::ApiMode::OpenAI,
            )),
            "/chat/completions",
            openai,
        ),
        "minimax-anthropic" => (
            Box::new(super::minimax::MiniMaxProvider::new(
                key,
                model,
                base,
                crate::config::ApiMode::Anthropic,
            )),
            "/v1/messages",
            anthropic,
        ),
        "mimo-anthropic" => (
            Box::new(super::mimo::MiMoProvider::new(
                key,
                model,
                base,
                crate::config::ApiMode::Anthropic,
            )),
            "/v1/messages",
            anthropic,
        ),
        _ => panic!("test adapter"),
    }
}
const ADAPTERS: &[&str] = &[
    "openai",
    "openrouter",
    "anthropic",
    "ollama",
    "llamacpp",
    "minimax",
    "mimo",
    "minimax-anthropic",
    "mimo-anthropic",
];

#[tokio::test]
async fn resilience_http_all_adapters_retry_429_and_keep_request_identical() {
    for name in ADAPTERS {
        let server = MockServer::start().await;
        let (p, endpoint, response) = adapter(name, server.uri());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(move |_: &wiremock::Request| {
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    ResponseTemplate::new(429)
                        .insert_header("Retry-After", "0")
                        .set_body_json(serde_json::json!({"error":{"message":"PRIVATE PROMPT"}}))
                } else {
                    ResponseTemplate::new(200).set_body_json(response.clone())
                }
            })
            .expect(2)
            .mount(&server)
            .await;
        let provider = p.name().to_string();
        let r = LLMRouter::with_providers(vec![p], provider, vec![], policy());
        assert_eq!(
            r.chat(request(), None).await.unwrap().content.as_deref(),
            Some("ok"),
            "{name}"
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].body, requests[1].body);
        if *name == "ollama" {
            assert_eq!(
                requests[0].body_json::<serde_json::Value>().unwrap()["options"]["num_predict"],
                10
            );
        }
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn resilience_http_all_adapters_reject_permanent_and_empty_responses() {
    for name in ADAPTERS {
        for (status, body) in [
            (400, serde_json::json!({"error":"PRIVATE PROMPT"})),
            (401, serde_json::json!({"error":"PRIVATE KEY"})),
            (200, serde_json::json!({})),
        ] {
            let server = MockServer::start().await;
            let (p, endpoint, _) = adapter(name, server.uri());
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;
            let provider = p.name().to_string();
            let r = LLMRouter::with_providers(vec![p], provider, vec![], policy());
            let e = r.chat(request(), None).await.unwrap_err().to_string();
            assert!(!e.contains("PRIVATE"), "{e}");
            assert_eq!(server.received_requests().await.unwrap().len(), 1, "{name}");
        }
    }
}

#[tokio::test]
async fn resilience_http_embedded_openrouter_error_is_not_empty_success() {
    let server = MockServer::start().await;
    let (p, endpoint, response) = adapter("openrouter", server.uri());
    let count = AtomicUsize::new(0);
    Mock::given(method("POST"))
        .and(path(endpoint))
        .respond_with(move |_: &wiremock::Request| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"error":{"code":503,"message":"PRIVATE"}}))
            } else {
                ResponseTemplate::new(200).set_body_json(response.clone())
            }
        })
        .expect(2)
        .mount(&server)
        .await;
    let r = LLMRouter::with_providers(vec![p], "openrouter".into(), vec![], policy());
    assert_eq!(
        r.chat(request(), None).await.unwrap().content.as_deref(),
        Some("ok")
    );
}

#[tokio::test]
async fn resilience_http_preserves_safe_diagnostics_and_quota_does_not_retry() {
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(429)
        .insert_header("Retry-After", "17").insert_header("x-request-id", "synthetic-request-1")
        .set_body_json(serde_json::json!({"error":{"code":"insufficient_quota","message":"SECRET-TOKEN"}})))
        .expect(1).mount(&server).await;
    let (p, _, _) = adapter("openai", server.uri());
    let r = LLMRouter::with_providers(vec![p], "openai".into(), vec![], policy());
    let e = r.chat(request(), None).await.unwrap_err().to_string();
    for expected in ["quota", "HTTP 429", "17s", "synthetic-request-1"] {
        assert!(e.contains(expected), "{e}");
    }
    assert!(!e.contains("SECRET"));
}

#[tokio::test]
async fn resilience_http_ollama_stream_retries_before_output_and_parses_usage() {
    let server = MockServer::start().await;
    let count = AtomicUsize::new(0);
    Mock::given(method("POST")).and(path("/api/chat")).respond_with(move |request: &wiremock::Request| {
        let body = request.body_json::<serde_json::Value>().unwrap();
        assert_eq!(body["stream"], true);
        assert_eq!(body["options"]["num_predict"], 10);
        if count.fetch_add(1, Ordering::SeqCst) == 0 { ResponseTemplate::new(503) }
        else { ResponseTemplate::new(200).set_body_string("{\"message\":{\"content\":\"日本語\"},\"done\":false}\n{\"done\":true,\"prompt_eval_count\":5,\"eval_count\":2}\n") }
    }).expect(2).mount(&server).await;
    let (p, _, _) = adapter("ollama", server.uri());
    let r = LLMRouter::with_providers(vec![p], "ollama".into(), vec![], policy());
    let mut events = crate::dashboard::stream::get_or_create("resilience-http-stream").subscribe();
    let response = r
        .streaming_chat(request(), None, "resilience-http-stream")
        .await
        .unwrap();
    assert_eq!(response.content.as_deref(), Some("日本語"));
    assert_eq!(response.usage.unwrap().total_tokens, 7);
    while let Ok(event) = events.try_recv() {
        assert_ne!(
            event.event, "stream_abort",
            "pre-output retry must not stop the task timer"
        );
    }
}

#[tokio::test]
async fn resilience_http_ollama_partial_stream_never_retries_or_emits_final() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("{\"message\":{\"content\":\"partial\"},\"done\":false}\n"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let (p, _, _) = adapter("ollama", server.uri());
    let r = LLMRouter::with_providers(vec![p], "ollama".into(), vec![], policy());
    let user = "resilience-http-partial";
    let mut events = crate::dashboard::stream::get_or_create(user).subscribe();
    assert!(r
        .streaming_chat(request(), None, user)
        .await
        .unwrap_err()
        .to_string()
        .contains("partial"));
    while let Ok(event) = events.try_recv() {
        assert_ne!(event.event, "assistant");
    }
}

#[tokio::test]
async fn resilience_transport_failure_diagnostics_never_expose_url_credentials() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let (p, _, _) = adapter(
        "openai",
        format!("http://fake-user:SECRET-certificate-TOKEN@127.0.0.1:{port}"),
    );
    let r = LLMRouter::with_providers(vec![p], "openai".into(), vec![], policy());
    let e = r.chat(request(), None).await.unwrap_err().to_string();
    assert!(
        e.contains("transport failure") && e.contains("connection refused"),
        "{e}"
    );
    assert!(e.contains("2 attempt(s)"), "{e}");
    for private in ["SECRET-certificate-TOKEN", "fake-user", "127.0.0.1"] {
        assert!(!e.contains(private), "{e}");
    }
}

#[tokio::test]
async fn llamacpp_reasoning_only_surfaces_on_streaming_path() {
    // Reproduces the reported qwen reasoning-model payload: the model spent the
    // whole output budget on its chain-of-thought, so content is empty and only
    // reasoning_content carries text, with finish_reason=length. This must NOT
    // raise InvalidResponse, and the reasoning must be surfaced as the reply on
    // the streaming path (llamacpp uses the default chat_stream -> chat()).
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "step-by-step reasoning that exhausted the budget"
                },
                "finish_reason": "length"
            }],
            "usage": {"prompt_tokens": 7, "completion_tokens": 4096, "total_tokens": 4103}
        })))
        .mount(&server)
        .await;
    let (p, _, _) = adapter("llamacpp", server.uri());
    let r = LLMRouter::with_providers(vec![p], "llamacpp".into(), vec![], policy());
    let response = r
        .streaming_chat(request(), None, "llamacpp-reasoning-only")
        .await
        .expect("reasoning-only reply must not raise InvalidResponse");
    assert_eq!(
        response.content.as_deref(),
        Some("step-by-step reasoning that exhausted the budget"),
        "reasoning must be written back into content, not discarded"
    );
    assert_eq!(response.finish_reason.as_deref(), Some("length"));
}

#[test]
fn resilience_router_rejects_empty_keys_and_shares_same_account_gates() {
    let mut config = crate::config::Config::from_env();
    config.use_provider = "openai".into();
    config.llm_resilience = policy();
    config.llm_fallback_providers = vec![];
    let mut secrets = crate::db::secrets::Secrets {
        openai_api_key: Some("  ".into()),
        ..Default::default()
    };
    assert!(LLMRouter::new(&config, &secrets)
        .validate_configuration()
        .is_err());
    config.openai_api_base = "http://127.0.0.1:9/v1".into();
    config.openrouter_api_base = "http://127.0.0.1:9/another/path".into();
    secrets.openai_api_key = Some("synthetic-account".into());
    secrets.openrouter_api_key = secrets.openai_api_key.clone();
    let r = LLMRouter::new(&config, &secrets);
    assert!(Arc::ptr_eq(&r.gates["openai"], &r.gates["openrouter"]));
    secrets.openrouter_api_key = Some("separate-account".into());
    let r = LLMRouter::new(&config, &secrets);
    assert!(!Arc::ptr_eq(&r.gates["openai"], &r.gates["openrouter"]));
}
