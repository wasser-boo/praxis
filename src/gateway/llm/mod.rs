pub mod anthropic;
pub mod minimax;
pub mod mimo;
pub mod ollama;
pub mod openai;
pub mod provider;

use provider::{ChatRequest, ChatResponse, LLMProvider};

pub struct LLMRouter {
    providers: Vec<Box<dyn LLMProvider>>,
    default_provider: String,
}

impl LLMRouter {
    pub fn new(config: &crate::config::Config, secrets: &crate::db::secrets::Secrets) -> Self {
        let mut providers: Vec<Box<dyn LLMProvider>> = Vec::new();

        if let Some(ref key) = secrets.openai_api_key {
            providers.push(Box::new(openai::OpenAIProvider::new(
                key.clone(),
                config.openai_model.clone(),
                config.openai_api_base.clone(),
            )));
        }

        if let Some(ref key) = secrets.anthropic_api_key {
            providers.push(Box::new(anthropic::AnthropicProvider::new(key.clone())));
        }

        providers.push(Box::new(ollama::OllamaProvider::new(
            config.ollama_api_base.clone(),
            config.ollama_model.clone(),
        )));

        if let Some(ref key) = secrets.minimax_api_key {
            providers.push(Box::new(minimax::MiniMaxProvider::new(key.clone())));
        }

        if let Some(ref key) = secrets.mimo_api_key {
            providers.push(Box::new(mimo::MiMoProvider::new(key.clone())));
        }

        Self {
            providers,
            default_provider: config.use_provider.clone(),
        }
    }

    pub async fn chat(
        &self,
        request: ChatRequest,
        provider: Option<&str>,
    ) -> anyhow::Result<ChatResponse> {
        let provider_name = provider.unwrap_or(&self.default_provider);

        for p in &self.providers {
            if p.name() == provider_name {
                match p.chat(request.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        tracing::warn!("Provider {} failed: {}", provider_name, e);
                        break;
                    }
                }
            }
        }

        for p in &self.providers {
            if p.name() != provider_name {
                match p.chat(request.clone()).await {
                    Ok(response) => {
                        tracing::info!("Fallback to {} successful", p.name());
                        return Ok(response);
                    }
                    Err(e) => {
                        tracing::warn!("Fallback {} failed: {}", p.name(), e);
                    }
                }
            }
        }

        Err(anyhow::anyhow!("All LLM providers failed"))
    }

    pub async fn health_check(&self) -> bool {
        for p in &self.providers {
            if p.health_check().await {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod gateway_tests {
    use super::*;

    #[test]
    fn test_llm_router_creation() {
        let config = crate::config::Config::from_env();
        let secrets = crate::db::secrets::Secrets::default();
        let _router = LLMRouter::new(&config, &secrets);
    }
}
