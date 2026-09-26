//! Renders every shipped POML template through the real POML CLI. Include
//! paths, raw `&&` in text and unknown variables only fail at render time,
//! so this is the only test that can catch them (bugs.md #8–#11).
//!
//! Gated: runs when `POML_CLI` points at the CLI; otherwise it is skipped with
//! a notice, unless `PRAXIS_REQUIRE_POML=1` makes the skip a failure (CI).
use std::path::{Path, PathBuf};

fn shipped_templates(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "poml") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("templates"), &mut out);
    out.sort();
    out
}

/// Partials under templates/shared and templates/personas are only rendered
/// through their including root templates.
fn is_partial(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root.join("templates")).unwrap();
    rel.starts_with("shared") || rel.starts_with("personas")
}

#[tokio::test]
async fn shipped_templates_render_with_the_real_poml_cli() {
    if std::env::var_os("POML_CLI").is_none() {
        if std::env::var("PRAXIS_REQUIRE_POML").ok().as_deref() == Some("1") {
            panic!("PRAXIS_REQUIRE_POML=1 but POML_CLI is unset");
        }
        eprintln!("skipping: set POML_CLI=<path to poml cli.js> to render shipped templates");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Database::new(dir.path()).unwrap();
    let plugins = crate::plugins::PluginRegistry::new();
    let mut ctx = crate::db::contexts::Context { user_id: "render-test".into(), ..Default::default() };
    ctx.sm_data = serde_json::json!({
        "role": "standard",
        "persona_roles": "standard, code, code_architect, senior_dev, expert_programmer, debugger, review, teach, research",
    });
    let base = super::prompt::build_context(&db, &ctx, "Synthetic request: explain the plan.", &plugins, 0, root).await.unwrap();

    let mut failures = Vec::new();
    let mut sizes = Vec::new();
    for path in shipped_templates(root).into_iter().filter(|p| !is_partial(root, p)) {
        let name = path.strip_prefix(root.join("templates")).unwrap().with_extension("");
        let mut context = base.clone();
        context["settings"]["system_template"] = serde_json::json!(name.to_string_lossy());
        context["system_template"] = context["settings"]["system_template"].clone();
        match super::poml::render_strict(&path.to_string_lossy(), &context).await {
            Ok(text) => sizes.push((name.to_string_lossy().into_owned(), text.chars().count())),
            Err(error) => failures.push(format!("{}: {error}", name.display())),
        }
    }
    assert!(failures.is_empty(), "templates failed to render:\n{}", failures.join("\n"));

    // Every state of every shipped workflow must select a renderable template.
    for entry in std::fs::read_dir(root.join("contexts")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "sm") { continue; }
        let sm = crate::sm::load_file_in(&root.join("contexts"), path.file_stem().unwrap().to_str().unwrap()).unwrap();
        for (state, vars) in &sm.states {
            if let Some(template) = vars.variables.get("settings.system_template") {
                assert!(sizes.iter().any(|(n, _)| n == template), "{}:{state} selects unrendered template {template}", path.display());
            }
        }
    }

    // The standard state must stay small enough for a 4096-token context
    // (≈4 chars/token; leaves room for tool schemas, history and the answer).
    for (name, chars) in &sizes {
        eprintln!("{name}: {chars} chars (~{} tokens)", chars / 4);
        if name == "states/standard/standard" || name == "standard" {
            assert!(*chars <= 2800, "{name} renders {chars} chars; keep the standard prompt under ~700 tokens");
        }
    }
}
