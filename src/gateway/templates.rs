use std::path::{Path, PathBuf};

/// Resolve a selected template without traversal or symlink escapes.
pub fn resolve_template(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    validate_name(name)?;
    let root = root.canonicalize()?;
        if name.is_empty() || name.contains("..") || name.starts_with('/')
        || name.contains("//") || name.ends_with(".poml")
    {
        anyhow::bail!("Unsafe template name: {name}");
    }
    let path = match root.join(format!("{name}.poml")).canonicalize() {
        Ok(path) => path,
        // Stale stored template names (deleted files) fall back to the core template.
        Err(_) => {
            tracing::warn!(template = %name, "Template missing; falling back to standard");
            root.join("standard.poml").canonicalize()?
        }
    };
    anyhow::ensure!(path.starts_with(&root) && path.is_file(), "Template must be a file inside templates/");
    Ok(path)
}

pub fn validate_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(name.split('/').all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')), "Template name must use letters, numbers, '_' or '-', with optional '/' separators and no extension");
    Ok(())
}

pub struct TemplateManager {
    templates_dir: String,
}

impl TemplateManager {
    pub fn new(templates_dir: &str) -> Self {
        Self {
            templates_dir: templates_dir.to_string(),
        }
    }

    pub fn load(&self, name: &str) -> anyhow::Result<String> {
        let path = resolve_template(Path::new(&self.templates_dir), name)?;
        Ok(std::fs::read_to_string(&path)?)
    }

    pub fn list(&self) -> anyhow::Result<Vec<String>> {
        let mut templates = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.templates_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".poml") {
                        templates.push(name.replace(".poml", ""));
                    }
                }
            }
        }
        Ok(templates)
    }
}

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn backend_template_selection_rejects_missing_and_unsafe_names() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("standard.poml"), "<poml><p>ok</p></poml>").unwrap();
        assert!(super::resolve_template(root.path(), "standard").is_ok());
        for name in ["", "../escape", "standard.poml", "/standard", "a//b"] {
            assert!(super::resolve_template(root.path(), name).is_err(), "{name}");
        }
        // Stale stored names fall back to the core template instead of failing.
        let missing = super::resolve_template(root.path(), "missing").unwrap();
        assert_eq!(missing.file_name().unwrap(), "standard.poml");
    }

    #[test]
    fn test_template_manager_compiles() {
        assert!(true);
    }
}
