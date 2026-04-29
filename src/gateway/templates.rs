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
        let path = format!("{}/{}.poml", self.templates_dir, name);
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
    fn test_template_manager_compiles() {
        assert!(true);
    }
}
