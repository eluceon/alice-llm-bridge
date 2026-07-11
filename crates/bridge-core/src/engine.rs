//! The dialogue engine: ties profiles, commands, prompts, storage and
//! providers into a single `handle` entry point.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{Datelike, NaiveDate, Utc};

use crate::command::{Command, Parsed, parse};
use crate::pending::{PendingAnswers, Poll};
use crate::phrases;
use crate::store::ConversationStore;
use crate::{FamilyRoster, Mode, ModelRegistry, ModelTier, UsageStats};

/// Tunables that come from configuration rather than the domain itself.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub context_window: usize,
    pub reply_budget: Duration,
    pub utc_offset_hours: i32,
}

#[derive(Debug, Clone)]
struct DialogueState {
    active_profile: String,
    mode: Option<String>,
    model_tier: ModelTier,
    window_override: Option<usize>,
}

struct EngineInner {
    roster: FamilyRoster,
    modes: Vec<Mode>,
    models: ModelRegistry,
    store: Arc<dyn ConversationStore>,
    pending: PendingAnswers,
    state: Mutex<DialogueState>,
    config: EngineConfig,
}

/// The skill's entry point: one call per Alice utterance.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<EngineInner>,
}

impl Engine {
    pub fn new(
        roster: FamilyRoster,
        modes: Vec<Mode>,
        models: ModelRegistry,
        store: Arc<dyn ConversationStore>,
        config: EngineConfig,
    ) -> Self {
        let state = DialogueState {
            active_profile: roster.default_profile().name.clone(),
            mode: None,
            model_tier: ModelTier::Fast,
            window_override: None,
        };
        Self {
            inner: Arc::new(EngineInner {
                roster,
                modes,
                models,
                store,
                pending: PendingAnswers::new(),
                state: Mutex::new(state),
                config,
            }),
        }
    }

    pub async fn handle(&self, user_id: &str, utterance: &str) -> String {
        match self.inner.pending.poll(user_id) {
            Poll::Answer(text) => return text,
            Poll::StillThinking => return phrases::PHRASE_STILL_THINKING.to_string(),
            Poll::None => {}
        }
        match parse(utterance, &self.inner.roster, &self.inner.modes) {
            Parsed::Command(command) => self.execute(user_id, utterance, command).await,
            Parsed::Ask { text, think_hard } => {
                let tier = think_hard.then_some(ModelTier::Smart);
                self.ask_llm(user_id, &text, tier).await
            }
        }
    }

    async fn execute(&self, user_id: &str, utterance: &str, command: Command) -> String {
        match command {
            Command::Introduce(name) => {
                self.inner
                    .state
                    .lock()
                    .expect("engine state mutex poisoned")
                    .active_profile = name.clone();
                format!("Привет, {name}! Слушаю.")
            }
            Command::Forget => {
                let profile = self.active_profile();
                match self.inner.store.clear_profile(&profile).await {
                    Ok(()) => "Всё, забыла наш разговор. Начинаем с чистого листа.".to_string(),
                    Err(err) => {
                        tracing::error!(error = %err, "failed to clear history");
                        phrases::PHRASE_INTERNAL_ERROR.to_string()
                    }
                }
            }
            Command::SetWindow(n) => {
                let n = n.clamp(1, 50);
                self.inner
                    .state
                    .lock()
                    .expect("engine state mutex poisoned")
                    .window_override = Some(n);
                format!("Хорошо, теперь помню последние {n} реплик.")
            }
            Command::UseSmartModel => {
                self.inner
                    .state
                    .lock()
                    .expect("engine state mutex poisoned")
                    .model_tier = ModelTier::Smart;
                "Переключилась на умную модель.".to_string()
            }
            Command::UseFastModel => {
                self.inner
                    .state
                    .lock()
                    .expect("engine state mutex poisoned")
                    .model_tier = ModelTier::Fast;
                "Переключилась на быструю модель.".to_string()
            }
            Command::UsageStats => self.usage_report().await,
            Command::WhoAmI => format!("Сейчас со мной говорит {}.", self.active_profile()),
            Command::EnterMode(name) => {
                self.inner
                    .state
                    .lock()
                    .expect("engine state mutex poisoned")
                    .mode = Some(name);
                self.ask_llm(user_id, utterance, None).await
            }
            Command::ExitMode => {
                self.inner
                    .state
                    .lock()
                    .expect("engine state mutex poisoned")
                    .mode = None;
                "Возвращаюсь в обычный режим.".to_string()
            }
            Command::Help => phrases::HELP_TEXT.to_string(),
        }
    }

    async fn ask_llm(
        &self,
        _user_id: &str,
        _question: &str,
        _tier_override: Option<ModelTier>,
    ) -> String {
        phrases::PHRASE_INTERNAL_ERROR.to_string()
    }

    async fn usage_report(&self) -> String {
        let offset = chrono::Duration::hours(self.inner.config.utc_offset_hours.into());
        let local_now = Utc::now() + offset;
        let day_start = local_now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("midnight is a valid time")
            .and_utc()
            - offset;
        let month_start = local_now
            .date_naive()
            .with_day(1)
            .expect("day 1 is a valid day")
            .and_hms_opt(0, 0, 0)
            .expect("midnight is a valid time")
            .and_utc()
            - offset;

        let today = self.inner.store.usage_since(day_start).await;
        let month = self.inner.store.usage_since(month_start).await;
        match (today, month) {
            (Ok(today), Ok(month)) => format!(
                "Сегодня {} ответов примерно на {} долларов. С начала месяца {} ответов на {} долларов.",
                today.requests,
                format_usd(today),
                month.requests,
                format_usd(month),
            ),
            (today, month) => {
                if let Err(err) = today.and(month) {
                    tracing::error!(error = %err, "failed to load usage stats");
                }
                phrases::PHRASE_INTERNAL_ERROR.to_string()
            }
        }
    }

    fn active_profile(&self) -> String {
        self.inner
            .state
            .lock()
            .expect("engine state mutex poisoned")
            .active_profile
            .clone()
    }

    #[allow(dead_code)]
    fn today(&self) -> NaiveDate {
        (Utc::now() + chrono::Duration::hours(self.inner.config.utc_offset_hours.into()))
            .date_naive()
    }
}

fn format_usd(stats: UsageStats) -> String {
    format!("{:.2}", stats.cost_micros as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{MemoryStore, ScriptedProvider};
    use crate::{
        ExchangeRecord, FamilyRoster, Mode, ModelPreset, ModelRegistry, Profile, ProfileRole,
    };
    use std::sync::Arc;
    use std::time::Duration;

    pub(super) fn preset(provider: Arc<ScriptedProvider>) -> ModelPreset {
        ModelPreset {
            provider,
            model: "test-model".to_string(),
            max_tokens: 300,
            temperature: 0.7,
            input_price_per_mtok: 1.0,
            output_price_per_mtok: 2.0,
        }
    }

    pub(super) fn engine_with(
        fast: Arc<ScriptedProvider>,
        smart: Arc<ScriptedProvider>,
        store: Arc<MemoryStore>,
    ) -> Engine {
        let profiles = vec![
            Profile {
                name: "Дима".to_string(),
                aliases: vec!["дима".to_string(), "папа".to_string()],
                birthday: None,
                role: ProfileRole::Adult,
                persona: String::new(),
            },
            Profile {
                name: "Маша".to_string(),
                aliases: vec!["маша".to_string()],
                birthday: None,
                role: ProfileRole::Child,
                persona: String::new(),
            },
        ];
        let roster = FamilyRoster::new(profiles, "Дима").unwrap();
        let modes = vec![Mode {
            name: "fairy_tale".to_string(),
            triggers: vec!["расскажи сказку".to_string()],
            prompt: "Рассказывай сказки.".to_string(),
        }];
        let models = ModelRegistry {
            fast: preset(fast),
            smart: preset(smart),
        };
        Engine::new(
            roster,
            modes,
            models,
            store,
            EngineConfig {
                context_window: 4,
                reply_budget: Duration::from_millis(2800),
                utc_offset_hours: 3,
            },
        )
    }

    pub(super) fn simple_engine() -> Engine {
        engine_with(
            ScriptedProvider::replying("ок"),
            ScriptedProvider::replying("умный ответ"),
            Arc::new(MemoryStore::new()),
        )
    }

    #[tokio::test]
    async fn introduce_switches_profile() {
        let engine = simple_engine();
        let reply = engine.handle("u1", "это Маша").await;
        assert!(reply.contains("Маша"));
        let who = engine.handle("u1", "кто я").await;
        assert!(who.contains("Маша"));
    }

    #[tokio::test]
    async fn forget_clears_history() {
        let store = Arc::new(MemoryStore::new());
        store
            .record_exchange(&ExchangeRecord {
                profile: "Дима".to_string(),
                user_text: "в".to_string(),
                assistant_text: "о".to_string(),
                model: "m".to_string(),
                prompt_tokens: 1,
                completion_tokens: 1,
                cost_micros: 1,
            })
            .await
            .unwrap();
        let engine = engine_with(
            ScriptedProvider::replying("ок"),
            ScriptedProvider::replying("ок"),
            store.clone(),
        );
        engine.handle("u1", "забудь всё").await;
        assert!(store.recent_messages("Дима", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn set_window_is_clamped_and_confirmed() {
        let engine = simple_engine();
        let reply = engine.handle("u1", "помни последние 500 реплик").await;
        assert!(reply.contains("50"));
    }

    #[tokio::test]
    async fn model_switch_is_confirmed() {
        let engine = simple_engine();
        let reply = engine.handle("u1", "переключись на умную модель").await;
        assert!(reply.contains("умную"));
    }

    #[tokio::test]
    async fn usage_stats_are_reported() {
        let store = Arc::new(MemoryStore::new());
        store
            .record_exchange(&ExchangeRecord {
                profile: "Дима".to_string(),
                user_text: "в".to_string(),
                assistant_text: "о".to_string(),
                model: "m".to_string(),
                prompt_tokens: 1,
                completion_tokens: 1,
                cost_micros: 2_000_000,
            })
            .await
            .unwrap();
        let engine = engine_with(
            ScriptedProvider::replying("ок"),
            ScriptedProvider::replying("ок"),
            store,
        );
        let reply = engine.handle("u1", "сколько потратили").await;
        assert!(reply.contains("1"), "reply: {reply}");
        assert!(reply.contains("2.00"), "reply: {reply}");
    }

    #[tokio::test]
    async fn help_is_returned() {
        let engine = simple_engine();
        assert_eq!(
            engine.handle("u1", "помощь").await,
            crate::phrases::HELP_TEXT
        );
    }
}
