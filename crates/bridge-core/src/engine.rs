//! The dialogue engine: ties profiles, commands, prompts, storage and
//! providers into a single `handle` entry point.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{Datelike, NaiveDate, Utc};
use llm_providers::{ChatMessage, ChatRequest, ProviderError};
use tokio::sync::oneshot;

use crate::command::{Command, Parsed, parse};
use crate::model::{ModelPreset, cost_micros};
use crate::pending::{PendingAnswers, Poll};
use crate::phrases;
use crate::prompt::{PromptContext, build_system_prompt};
use crate::store::{ConversationStore, ExchangeRecord, MessageRole};
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
        user_id: &str,
        question: &str,
        tier_override: Option<ModelTier>,
    ) -> String {
        let (profile_name, mode_name, state_tier, window) = {
            let state = self
                .inner
                .state
                .lock()
                .expect("engine state mutex poisoned");
            (
                state.active_profile.clone(),
                state.mode.clone(),
                state.model_tier,
                state
                    .window_override
                    .unwrap_or(self.inner.config.context_window),
            )
        };
        let tier = tier_override.unwrap_or(state_tier);

        let Some(profile) = self.inner.roster.get(&profile_name).cloned() else {
            tracing::error!(profile = %profile_name, "active profile missing from roster");
            return phrases::PHRASE_INTERNAL_ERROR.to_string();
        };
        let mode = mode_name
            .as_deref()
            .and_then(|name| self.inner.modes.iter().find(|m| m.name == name))
            .cloned();

        let summary = self
            .inner
            .store
            .summary(&profile_name)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!(error = %err, "failed to load summary");
                None
            });
        let history = self
            .inner
            .store
            .recent_messages(&profile_name, window)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!(error = %err, "failed to load history");
                Vec::new()
            });

        let system = build_system_prompt(&PromptContext {
            today: self.today(),
            profile: &profile,
            roster: &self.inner.roster,
            mode: mode.as_ref(),
            summary: summary.as_ref().map(|s| s.content.as_str()),
        });
        let mut messages = Vec::with_capacity(history.len() + 2);
        messages.push(ChatMessage::system(system));
        for message in history {
            messages.push(match message.role {
                MessageRole::User => ChatMessage::user(message.content),
                MessageRole::Assistant => ChatMessage::assistant(message.content),
            });
        }
        messages.push(ChatMessage::user(question));

        let preset = self.inner.models.get(tier).clone();

        // Mark before spawning so a completion that races the deadline always
        // finds a slot to land in; the fast path clears the mark on delivery.
        self.inner.pending.mark_thinking(user_id);
        let (sender, receiver) = oneshot::channel();
        let engine = self.clone();
        let question = question.to_string();
        let key = user_id.to_string();
        tokio::spawn(async move {
            let reply = engine
                .generate(&profile_name, &question, &preset, messages)
                .await;
            if let Err(reply) = sender.send(reply) {
                engine.inner.pending.complete(&key, reply);
            }
        });

        match tokio::time::timeout(self.inner.config.reply_budget, receiver).await {
            Ok(Ok(reply)) => {
                self.inner.pending.clear(user_id);
                reply
            }
            Ok(Err(_closed)) => {
                self.inner.pending.clear(user_id);
                tracing::error!("reply task dropped its channel");
                phrases::PHRASE_INTERNAL_ERROR.to_string()
            }
            Err(_deadline) => phrases::PHRASE_THINKING_STARTED.to_string(),
        }
    }

    async fn generate(
        &self,
        profile: &str,
        question: &str,
        preset: &ModelPreset,
        messages: Vec<ChatMessage>,
    ) -> String {
        let request = ChatRequest {
            model: preset.model.clone(),
            messages,
            max_tokens: preset.max_tokens,
            temperature: preset.temperature,
        };
        match preset.provider.chat(&request).await {
            Ok(completion) => {
                let record = ExchangeRecord {
                    profile: profile.to_string(),
                    user_text: question.to_string(),
                    assistant_text: completion.text.clone(),
                    model: preset.model.clone(),
                    prompt_tokens: completion.usage.prompt_tokens,
                    completion_tokens: completion.usage.completion_tokens,
                    cost_micros: cost_micros(&completion.usage, preset),
                };
                if let Err(err) = self.inner.store.record_exchange(&record).await {
                    tracing::error!(error = %err, "failed to record exchange");
                }
                let engine = self.clone();
                let profile_name = profile.to_string();
                tokio::spawn(async move {
                    engine.maybe_summarize(&profile_name).await;
                });
                completion.text
            }
            Err(err) => {
                tracing::warn!(error = %err, "chat request failed");
                phrase_for_provider_error(&err).to_string()
            }
        }
    }

    /// Compresses history older than the active window into a rolling summary
    /// so long-lived profiles keep cheap prompts.
    async fn maybe_summarize(&self, profile: &str) {
        let keep_last = {
            let state = self
                .inner
                .state
                .lock()
                .expect("engine state mutex poisoned");
            state
                .window_override
                .unwrap_or(self.inner.config.context_window)
        };
        let pending = match self.inner.store.unsummarized(profile, keep_last).await {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!(error = %err, "failed to load unsummarized history");
                return;
            }
        };
        if pending.len() < keep_last {
            return;
        }

        let previous = self
            .inner
            .store
            .summary(profile)
            .await
            .ok()
            .flatten()
            .map(|s| s.content)
            .unwrap_or_default();
        let transcript: String = pending
            .iter()
            .map(|(_, m)| {
                let speaker = match m.role {
                    MessageRole::User => "Пользователь",
                    MessageRole::Assistant => "Ассистент",
                };
                format!("{speaker}: {}\n", m.content)
            })
            .collect();
        let instruction = format!(
            "Сожми диалог в краткое резюме из трёх-пяти предложений. \
             Сохрани важные факты о собеседнике и темы разговора.\n\
             Текущее резюме: {previous}\nНовые реплики:\n{transcript}"
        );

        let preset = self.inner.models.get(ModelTier::Fast).clone();
        let request = ChatRequest {
            model: preset.model.clone(),
            messages: vec![ChatMessage::system(instruction)],
            max_tokens: 250,
            temperature: 0.3,
        };
        let content = match preset.provider.chat(&request).await {
            Ok(completion) => completion.text,
            Err(err) => {
                tracing::warn!(error = %err, "summarization request failed");
                return;
            }
        };
        let covers_until = pending.iter().map(|(id, _)| *id).max().unwrap_or(0);
        let summary = crate::store::Summary {
            content,
            covers_until_message_id: covers_until,
        };
        if let Err(err) = self.inner.store.upsert_summary(profile, &summary).await {
            tracing::warn!(error = %err, "failed to store summary");
        }
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

    fn today(&self) -> NaiveDate {
        (Utc::now() + chrono::Duration::hours(self.inner.config.utc_offset_hours.into()))
            .date_naive()
    }
}

fn format_usd(stats: UsageStats) -> String {
    format!("{:.2}", stats.cost_micros as f64 / 1_000_000.0)
}

fn phrase_for_provider_error(err: &ProviderError) -> &'static str {
    match err {
        ProviderError::Timeout => phrases::PHRASE_PROVIDER_TIMEOUT,
        ProviderError::RateLimited => phrases::PHRASE_RATE_LIMITED,
        ProviderError::Auth => phrases::PHRASE_AUTH_FAILED,
        ProviderError::Api { .. }
        | ProviderError::Network(_)
        | ProviderError::InvalidResponse(_) => phrases::PHRASE_PROVIDER_ERROR,
    }
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

    #[tokio::test]
    async fn fast_answer_is_returned_directly_and_recorded() {
        let store = Arc::new(MemoryStore::new());
        let fast = ScriptedProvider::replying("Марс — четвёртая планета.");
        let engine = engine_with(fast.clone(), ScriptedProvider::replying("x"), store.clone());

        let reply = engine.handle("u1", "расскажи про марс").await;
        assert_eq!(reply, "Марс — четвёртая планета.");

        let messages = store.recent_messages("Дима", 10).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "расскажи про марс");
        assert_eq!(messages[1].content, "Марс — четвёртая планета.");

        let calls = fast.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].messages[0].content.contains("голосовой помощник"));
    }

    #[tokio::test(start_paused = true)]
    async fn slow_answer_is_deferred_then_delivered() {
        let store = Arc::new(MemoryStore::new());
        let fast = ScriptedProvider::slow("долгий ответ", Duration::from_secs(10));
        let engine = engine_with(fast, ScriptedProvider::replying("x"), store);

        let first = engine.handle("u1", "сложный вопрос").await;
        assert_eq!(first, crate::phrases::PHRASE_THINKING_STARTED);

        let second = engine.handle("u1", "ну что").await;
        assert_eq!(second, crate::phrases::PHRASE_STILL_THINKING);

        tokio::time::sleep(Duration::from_secs(11)).await;
        let third = engine.handle("u1", "ну что там").await;
        assert_eq!(third, "долгий ответ");
    }

    #[tokio::test]
    async fn think_hard_uses_smart_model() {
        let fast = ScriptedProvider::replying("быстрый");
        let smart = ScriptedProvider::replying("умный");
        let engine = engine_with(fast.clone(), smart.clone(), Arc::new(MemoryStore::new()));

        let reply = engine
            .handle("u1", "подумай как следует что такое энтропия")
            .await;
        assert_eq!(reply, "умный");
        assert_eq!(fast.calls.lock().unwrap().len(), 0);
        assert_eq!(smart.calls.lock().unwrap().len(), 1);
        assert_eq!(
            smart.calls.lock().unwrap()[0]
                .messages
                .last()
                .unwrap()
                .content,
            "что такое энтропия"
        );
    }

    #[tokio::test]
    async fn entering_mode_asks_llm_with_mode_prompt() {
        let fast = ScriptedProvider::replying("Жил-был кот.");
        let engine = engine_with(
            fast.clone(),
            ScriptedProvider::replying("x"),
            Arc::new(MemoryStore::new()),
        );

        let reply = engine.handle("u1", "расскажи сказку").await;
        assert_eq!(reply, "Жил-был кот.");
        let calls = fast.calls.lock().unwrap();
        assert!(calls[0].messages[0].content.contains("Рассказывай сказки."));
    }

    #[tokio::test]
    async fn provider_errors_become_human_phrases() {
        for (kind, phrase) in [
            ("timeout", crate::phrases::PHRASE_PROVIDER_TIMEOUT),
            ("rate", crate::phrases::PHRASE_RATE_LIMITED),
            ("auth", crate::phrases::PHRASE_AUTH_FAILED),
            ("boom", crate::phrases::PHRASE_PROVIDER_ERROR),
        ] {
            let engine = engine_with(
                ScriptedProvider::failing(kind),
                ScriptedProvider::replying("x"),
                Arc::new(MemoryStore::new()),
            );
            assert_eq!(engine.handle("u1", "вопрос").await, phrase, "kind: {kind}");
        }
    }

    #[tokio::test]
    async fn history_is_included_in_prompt() {
        let store = Arc::new(MemoryStore::new());
        let fast = ScriptedProvider::replying("ответ");
        let engine = engine_with(fast.clone(), ScriptedProvider::replying("x"), store);

        engine.handle("u1", "первый вопрос").await;
        engine.handle("u1", "второй вопрос").await;

        let calls = fast.calls.lock().unwrap();
        let second_call = &calls[1];
        // system + 2 history turns + new question
        assert_eq!(second_call.messages.len(), 4);
        assert_eq!(second_call.messages[1].content, "первый вопрос");
        assert_eq!(second_call.messages[2].content, "ответ");
    }

    #[tokio::test]
    async fn old_history_is_summarized_in_background() {
        let store = Arc::new(MemoryStore::new());
        let fast = ScriptedProvider::replying("сжатое резюме");
        // window = 4 (see engine_with); 3 exchanges = 6 messages -> 2 beyond window,
        // not enough; 4 exchanges = 8 messages -> 4 beyond window >= window -> summarize
        let engine = engine_with(fast.clone(), ScriptedProvider::replying("x"), store.clone());

        for n in 1..=4 {
            engine.handle("u1", &format!("вопрос {n}")).await;
        }
        // The summarization request is spawned after each exchange is
        // recorded; give the scheduler real time to run it.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let summary = store
            .summary("Дима")
            .await
            .unwrap()
            .expect("summary written");
        assert_eq!(summary.content, "сжатое резюме");
        assert!(summary.covers_until_message_id >= 4);

        let calls = fast.calls.lock().unwrap();
        let summarize_call = calls
            .iter()
            .find(|c| c.max_tokens == 250)
            .expect("summarization request sent");
        assert!(
            summarize_call.messages[0].content.contains("резюме")
                || summarize_call.messages[0].content.contains("Сожми")
        );
    }
}
