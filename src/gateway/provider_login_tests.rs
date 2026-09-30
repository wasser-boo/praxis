use super::*;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn endpoint_probe_rejects_generic_404_html_and_wrong_json() {
    for (status, body) in [
        (404, "not found"),
        (200, "<html>wrong server</html>"),
        (200, "{}"),
        (401, "{}"),
        (500, "{}"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;
        for provider in ["ollama", "llamacpp"] {
            assert!(
                !probe_endpoint(provider, &server.uri(), None).await,
                "{provider} accepted {status}: {body}"
            );
        }
    }
}

#[tokio::test]
async fn endpoint_probe_checks_models_and_legacy_llamacpp_health() {
    let server = MockServer::start().await;
    Mock::given(path("/v1/models"))
        .and(header("authorization", "Bearer llama-key"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(path("/health"))
        .and(header("authorization", "Bearer llama-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status":"ok"})))
        .mount(&server)
        .await;
    assert!(
        probe_endpoint(
            "llamacpp",
            &format!("{}/v1", server.uri()),
            Some("llama-key")
        )
        .await
    );
    assert!(
        !probe_endpoint(
            "llamacpp",
            &format!("{}/v1", server.uri()),
            Some("wrong-key")
        )
        .await
    );
    let server = MockServer::start().await;
    Mock::given(path("/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data":[{"id":"test-model"}]})),
        )
        .mount(&server)
        .await;
    assert!(probe_endpoint("llamacpp", &server.uri(), None).await);
}

#[test]
fn setup_selects_provider_and_model_together() {
    let state = tests::state();
    assert!(setup_hints(&state, "codex", "gpt-5-codex")[0]
        .contains("settings.provider=codex settings.model=gpt-5-codex"));
}
