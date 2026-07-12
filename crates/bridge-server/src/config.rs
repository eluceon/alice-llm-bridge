//! TOML configuration schema and validation.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;

use serde::Deserialize;

/// Failures while loading or validating the configuration file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("environment variable {0} is not set")]
    MissingEnv(String),
    #[error("model {model} references unknown provider {provider}")]
    UnknownProvider { model: String, provider: String },
    #[error("invalid config: {0}")]
    Invalid(String),
}

/// Root of `config.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub defaults: DefaultsConfig,
    pub providers: HashMap<String, ProviderConfig>,
    pub models: ModelsConfig,
    pub profiles: Vec<ProfileConfig>,
    #[serde(default)]
    pub modes: Vec<ModeConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    /// Yandex account `user_id`s allowed to use the skill; empty means
    /// unrestricted (the draft-skill visibility is the only gate).
    #[serde(default)]
    pub allowed_user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultsConfig {
    pub profile: String,
    #[serde(default = "default_context_window")]
    pub context_window: usize,
    #[serde(default = "default_reply_budget_ms")]
    pub reply_budget_ms: u64,
    #[serde(default = "default_provider_timeout_secs")]
    pub provider_timeout_secs: u64,
    #[serde(default = "default_utc_offset_hours")]
    pub utc_offset_hours: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub base_url: String,
    /// Name of the environment variable holding the API key.
    pub api_key_env: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelsConfig {
    pub fast: ModelPresetConfig,
    pub smart: ModelPresetConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPresetConfig {
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    pub input_price_per_mtok: f64,
    pub output_price_per_mtok: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub birthday: Option<chrono::NaiveDate>,
    /// `"adult"` or `"child"`.
    pub role: String,
    #[serde(default)]
    pub persona: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeConfig {
    pub name: String,
    pub triggers: Vec<String>,
    pub prompt: String,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        Self::from_toml(&std::fs::read_to_string(path)?)
    }

    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let config: AppConfig = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        for preset in [&self.models.fast, &self.models.smart] {
            if !self.providers.contains_key(&preset.provider) {
                return Err(ConfigError::UnknownProvider {
                    model: preset.model.clone(),
                    provider: preset.provider.clone(),
                });
            }
        }
        if !self
            .profiles
            .iter()
            .any(|p| p.name == self.defaults.profile)
        {
            return Err(ConfigError::Invalid(format!(
                "default profile {} is not defined",
                self.defaults.profile
            )));
        }
        for profile in &self.profiles {
            if profile.role != "adult" && profile.role != "child" {
                return Err(ConfigError::Invalid(format!(
                    "profile {}: role must be adult or child",
                    profile.name
                )));
            }
        }
        Ok(())
    }
}

fn default_context_window() -> usize {
    12
}
fn default_reply_budget_ms() -> u64 {
    2800
}
fn default_provider_timeout_secs() -> u64 {
    45
}
fn default_utc_offset_hours() -> i32 {
    3
}
fn default_temperature() -> f32 {
    0.7
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        r#"
[server]
listen = "127.0.0.1:8080"
allowed_user_ids = ["USER1"]

[defaults]
profile = "Дима"

[providers.deepseek]
base_url = "https://api.deepseek.com/v1"
api_key_env = "TEST_DEEPSEEK_KEY"

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
birthday = "1985-03-10"
role = "adult"
persona = "Общайся на равных."

[[modes]]
name = "fairy_tale"
triggers = ["расскажи сказку"]
prompt = "Рассказывай сказки."
"#
        .to_string()
    }

    #[test]
    fn parses_full_config_with_defaults() {
        let config = AppConfig::from_toml(&sample()).unwrap();
        assert_eq!(config.defaults.context_window, 12);
        assert_eq!(config.defaults.reply_budget_ms, 2800);
        assert_eq!(config.defaults.utc_offset_hours, 3);
        assert_eq!(config.models.fast.temperature, 0.7);
        assert_eq!(config.profiles[0].name, "Дима");
        assert_eq!(config.profiles[0].role, "adult");
        assert_eq!(config.modes.len(), 1);
    }

    #[test]
    fn rejects_model_with_unknown_provider() {
        let broken = sample().replace(r#"provider = "deepseek""#, r#"provider = "nope""#);
        let err = AppConfig::from_toml(&broken).unwrap_err();
        assert!(matches!(err, ConfigError::UnknownProvider { .. }));
    }

    #[test]
    fn rejects_unknown_default_profile() {
        let broken = sample().replace(r#"profile = "Дима""#, r#"profile = "Вася""#);
        let err = AppConfig::from_toml(&broken).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid(_)));
    }
}
