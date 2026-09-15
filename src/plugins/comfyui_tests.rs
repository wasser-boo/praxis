use super::*;
use crate::db::secrets::Secrets;
use serde_json::json;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn registry() -> PluginRegistry {
    load_all_plugins(&Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins"))
}

#[test]
fn comfyui_plugin_registers_discoverable_script_tools_and_scoped_secrets() {
    let registry = registry();
    let plugin = registry.get("comfyui").unwrap();
    assert!(plugin.enabled);
    assert_eq!(plugin.tools.len(), 4);
    for name in [
        "comfyui_nodes",
        "comfyui_workflow",
        "comfyui_run",
        "comfyui_result",
    ] {
        let tool = plugin.tools.iter().find(|t| t.name == name).unwrap();
        assert_eq!(
            registry
                .tool_definitions()
                .iter()
                .filter(|t| t.function.name == name)
                .count(),
            1
        );
        assert_eq!(tool.parameters["additionalProperties"], false);
        match &tool.handler {
            PluginHandler::Script { path, interpreter } => {
                assert!(Path::new(path).is_file());
                assert_eq!(interpreter, "python3");
            }
            _ => panic!("ComfyUI must use the existing standalone script handler"),
        }
    }
    let mut secrets = Secrets {
        gateway_api_key: Some("private-gateway-key".into()),
        ..Default::default()
    };
    secrets
        .custom
        .insert("comfyui_api_key".into(), "comfy-token".into());
    secrets
        .custom
        .insert("unrelated_key".into(), "not-for-comfyui".into());
    assert_eq!(
        registry.secrets_for_tool("comfyui_run", &secrets),
        HashMap::from([("comfyui_api_key".into(), "comfy-token".into())])
    );
    assert_eq!(registry.context_defaults()["comfyui_workflow_base_url"], "");
}

#[tokio::test]
async fn comfyui_plugin_executes_through_existing_registry_and_honors_disabled_state() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/object_info/LoadImage"))
        .and(header("authorization", "Bearer comfy-token"))
        .and(header("comfy-user", "profile-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "LoadImage": {
                "input": {"required": {"image": [["example.png"]]}},
                "output": ["IMAGE", "MASK"]
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    let mut registry = registry();
    let context = json!({
        "comfyui_workflow_base_url": server.uri(),
        "comfyui_workflow_user": "profile-1"
    });
    let credentials = HashMap::from([("comfyui_api_key".into(), "comfy-token".into())]);
    let args = json!({"node_class": "LoadImage"});
    let result = registry
        .execute_tool("comfyui_nodes", &args, Some(&context), Some(&credentials))
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["nodes"]["LoadImage"]["output"][0], "IMAGE");
    registry.plugins.get_mut("comfyui").unwrap().enabled = false;
    assert!(!registry
        .enabled_tools()
        .iter()
        .any(|tool| tool.name.starts_with("comfyui_")));
    assert!(registry
        .execute_tool("comfyui_nodes", &args, Some(&context), Some(&credentials))
        .await
        .is_err());
    server.verify().await;
}
