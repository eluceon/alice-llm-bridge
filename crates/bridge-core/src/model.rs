//! Named model presets and cost accounting.

use std::sync::Arc;

use llm_providers::{ChatProvider, TokenUsage};

/// Which quality/price point to use for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    Fast,
    Smart,
}

/// A concrete model behind a provider, with its limits and prices.
#[derive(Clone)]
pub struct ModelPreset {
    pub provider: Arc<dyn ChatProvider>,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    /// USD per million prompt tokens.
    pub input_price_per_mtok: f64,
    /// USD per million completion tokens.
    pub output_price_per_mtok: f64,
}

/// The two presets every installation defines.
pub struct ModelRegistry {
    pub fast: ModelPreset,
    pub smart: ModelPreset,
}

impl ModelRegistry {
    pub fn get(&self, tier: ModelTier) -> &ModelPreset {
        match tier {
            ModelTier::Fast => &self.fast,
            ModelTier::Smart => &self.smart,
        }
    }
}

/// Prices are USD per million tokens, so token count times price is exactly
/// the cost in micro-dollars.
pub fn cost_micros(usage: &TokenUsage, preset: &ModelPreset) -> i64 {
    (usage.prompt_tokens as f64 * preset.input_price_per_mtok
        + usage.completion_tokens as f64 * preset.output_price_per_mtok)
        .round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScriptedProvider;
    use llm_providers::TokenUsage;

    fn preset(input: f64, output: f64) -> ModelPreset {
        ModelPreset {
            provider: ScriptedProvider::replying("ok"),
            model: "m".to_string(),
            max_tokens: 100,
            temperature: 0.7,
            input_price_per_mtok: input,
            output_price_per_mtok: output,
        }
    }

    #[test]
    fn computes_cost_in_micro_dollars() {
        let usage = TokenUsage {
            prompt_tokens: 1_000_000,
            completion_tokens: 0,
        };
        assert_eq!(cost_micros(&usage, &preset(0.27, 1.10)), 270_000);

        let usage = TokenUsage {
            prompt_tokens: 1000,
            completion_tokens: 500,
        };
        // 1000 * 0.27 + 500 * 1.10 = 270 + 550 = 820 micro-dollars
        assert_eq!(cost_micros(&usage, &preset(0.27, 1.10)), 820);
    }

    #[test]
    fn registry_selects_tier() {
        let registry = ModelRegistry {
            fast: preset(0.1, 0.2),
            smart: preset(1.0, 2.0),
        };
        assert_eq!(registry.get(ModelTier::Fast).input_price_per_mtok, 0.1);
        assert_eq!(registry.get(ModelTier::Smart).input_price_per_mtok, 1.0);
    }
}
