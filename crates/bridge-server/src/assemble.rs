//! Builds a [`bridge_core::Engine`] from validated configuration, resolving
//! provider API keys from the environment.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bridge_core::{
    ConversationStore, Engine, EngineConfig, FamilyRoster, Mode, ModelPreset, ModelRegistry,
    Profile, ProfileRole,
};
use llm_providers::{ChatProvider, OpenAiCompatClient};

use crate::config::{AppConfig, ConfigError, ModelPresetConfig};

pub fn build_engine(
    config: &AppConfig,
    store: Arc<dyn ConversationStore>,
) -> Result<Engine, ConfigError> {
    let timeout = Duration::from_secs(config.defaults.provider_timeout_secs);
    let mut providers: HashMap<&str, Arc<dyn ChatProvider>> = HashMap::new();
    for (name, provider) in &config.providers {
        let api_key = std::env::var(&provider.api_key_env)
            .map_err(|_| ConfigError::MissingEnv(provider.api_key_env.clone()))?;
        let client = OpenAiCompatClient::new(&provider.base_url, api_key, timeout)
            .map_err(|e| ConfigError::Invalid(format!("provider {name}: {e}")))?;
        providers.insert(name.as_str(), Arc::new(client));
    }

    let registry = ModelRegistry {
        fast: preset(&config.models.fast, &providers)?,
        smart: preset(&config.models.smart, &providers)?,
    };

    let profiles = config
        .profiles
        .iter()
        .map(|p| Profile {
            name: p.name.clone(),
            aliases: p.aliases.iter().map(|a| a.to_lowercase()).collect(),
            birthday: p.birthday,
            role: if p.role == "child" {
                ProfileRole::Child
            } else {
                ProfileRole::Adult
            },
            persona: p.persona.clone(),
        })
        .collect();
    let roster = FamilyRoster::new(profiles, &config.defaults.profile)
        .map_err(|e| ConfigError::Invalid(e.to_string()))?;

    let modes = config
        .modes
        .iter()
        .map(|m| Mode {
            name: m.name.clone(),
            triggers: m.triggers.clone(),
            prompt: m.prompt.clone(),
        })
        .collect();

    Ok(Engine::new(
        roster,
        modes,
        registry,
        store,
        EngineConfig {
            context_window: config.defaults.context_window,
            reply_budget: Duration::from_millis(config.defaults.reply_budget_ms),
            utc_offset_hours: config.defaults.utc_offset_hours,
        },
    ))
}

fn preset(
    config: &ModelPresetConfig,
    providers: &HashMap<&str, Arc<dyn ChatProvider>>,
) -> Result<ModelPreset, ConfigError> {
    let provider = providers
        .get(config.provider.as_str())
        .ok_or_else(|| ConfigError::UnknownProvider {
            model: config.model.clone(),
            provider: config.provider.clone(),
        })?
        .clone();
    Ok(ModelPreset {
        provider,
        model: config.model.clone(),
        max_tokens: config.max_tokens,
        temperature: config.temperature,
        input_price_per_mtok: config.input_price_per_mtok,
        output_price_per_mtok: config.output_price_per_mtok,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use bridge_core::testing::MemoryStore;
    use std::sync::Arc;

    const CONFIG: &str = r#"
[server]
listen = "127.0.0.1:8080"
allowed_user_ids = []

[defaults]
profile = "Дима"

[providers.deepseek]
base_url = "https://api.deepseek.com/v1"
api_key_env = "TEST_DEEPSEEK_KEY_ASSEMBLE"

[models.fast]
provider = "deepseek"
model = "deepseek-chat"
max_tokens = 300
input_price_per_mtok = 0.27
output_price_per_mtok = 1.10

[models.smart]
provider = "deepseek"
model = "deepseek-reasoner"
max_tokens = 400
input_price_per_mtok = 0.55
output_price_per_mtok = 2.19

[[profiles]]
name = "Дима"
aliases = ["дима"]
role = "adult"
persona = ""
"#;

    #[test]
    fn builds_engine_when_key_env_present() {
        unsafe { std::env::set_var("TEST_DEEPSEEK_KEY_ASSEMBLE", "k") };
        let config = AppConfig::from_toml(CONFIG).unwrap();
        assert!(build_engine(&config, Arc::new(MemoryStore::new())).is_ok());
    }

    #[test]
    fn fails_without_key_env() {
        let config = AppConfig::from_toml(
            &CONFIG.replace("TEST_DEEPSEEK_KEY_ASSEMBLE", "TEST_DEEPSEEK_KEY_MISSING"),
        )
        .unwrap();
        let result = build_engine(&config, Arc::new(MemoryStore::new()));
        assert!(matches!(
            result,
            Err(crate::config::ConfigError::MissingEnv(_))
        ));
    }
}
