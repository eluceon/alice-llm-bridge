//! Recognition of control phrases that must never reach the model.

use crate::{FamilyRoster, Mode};

/// A control command spoken by the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Switch the active profile to the named family member.
    Introduce(String),
    /// Erase the active profile's history and summary.
    Forget,
    /// Change how many recent turns are sent to the model.
    SetWindow(usize),
    UseSmartModel,
    UseFastModel,
    UsageStats,
    WhoAmI,
    /// Activate the named mode; the utterance itself is still answered.
    EnterMode(String),
    ExitMode,
    Help,
}

/// Result of looking at an utterance: either a control command or a
/// question for the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Command(Command),
    Ask { text: String, think_hard: bool },
}

/// Lowercases, folds `ё` to `е`, strips punctuation and collapses whitespace,
/// so voice transcription variants compare equal.
pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .replace('ё', "е")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Classifies an utterance against the known commands, profiles and modes.
pub fn parse(utterance: &str, roster: &FamilyRoster, modes: &[Mode]) -> Parsed {
    let norm = normalize(utterance);
    if norm.is_empty() {
        return Parsed::Command(Command::Help);
    }

    for prefix in ["это ", "я ", "меня зовут ", "говорит "] {
        if let Some(rest) = norm.strip_prefix(prefix) {
            if let Some(profile) = roster.find_by_alias(rest) {
                return Parsed::Command(Command::Introduce(profile.name.clone()));
            }
        }
    }

    match norm.as_str() {
        "забудь" | "забудь все" | "сбрось контекст" | "начни сначала" =>
        {
            return Parsed::Command(Command::Forget);
        }
        "сколько потратили" | "сколько мы потратили" | "статистика" | "расходы" =>
        {
            return Parsed::Command(Command::UsageStats);
        }
        "кто я" | "с кем ты говоришь" => return Parsed::Command(Command::WhoAmI),
        "хватит" | "выйди из режима" | "обычный режим" => {
            return Parsed::Command(Command::ExitMode);
        }
        "помощь" | "справка" | "что ты умеешь" => {
            return Parsed::Command(Command::Help);
        }
        _ => {}
    }

    if norm.starts_with("помни последние") || norm.starts_with("запоминай последние")
    {
        if let Some(n) = norm
            .split_whitespace()
            .find_map(|w| w.parse::<usize>().ok())
        {
            return Parsed::Command(Command::SetWindow(n));
        }
    }

    if norm.contains("умную модель") || norm.contains("умная модель") {
        return Parsed::Command(Command::UseSmartModel);
    }
    if norm.contains("быструю модель")
        || norm.contains("быстрая модель")
        || norm.contains("модель на быструю")
    {
        return Parsed::Command(Command::UseFastModel);
    }

    for mode in modes {
        if mode.triggers.iter().any(|trigger| {
            let trigger = normalize(trigger);
            norm == trigger || norm.starts_with(&format!("{trigger} "))
        }) {
            return Parsed::Command(Command::EnterMode(mode.name.clone()));
        }
    }

    for prefix in [
        "подумай как следует",
        "подумай хорошенько",
        "подумай крепко",
    ] {
        if let Some(rest) = norm.strip_prefix(prefix) {
            let rest = rest.trim();
            let text = if rest.is_empty() {
                utterance.trim().to_string()
            } else {
                rest.to_string()
            };
            return Parsed::Ask {
                text,
                think_hard: true,
            };
        }
    }

    Parsed::Ask {
        text: utterance.trim().to_string(),
        think_hard: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FamilyRoster, Mode, Profile, ProfileRole};

    fn roster() -> FamilyRoster {
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
        FamilyRoster::new(profiles, "Дима").unwrap()
    }

    fn modes() -> Vec<Mode> {
        vec![Mode {
            name: "fairy_tale".to_string(),
            triggers: vec!["расскажи сказку".to_string(), "сказка".to_string()],
            prompt: "Расскажи короткую добрую сказку.".to_string(),
        }]
    }

    fn parse_one(utterance: &str) -> Parsed {
        parse(utterance, &roster(), &modes())
    }

    #[test]
    fn normalizes_text() {
        assert_eq!(
            normalize("Привет, Алиса! Ёлки-палки."),
            "привет алиса елки палки"
        );
    }

    #[test]
    fn recognizes_introductions() {
        for phrase in ["это Маша", "я маша", "меня зовут Маша", "говорит маша"]
        {
            let Parsed::Command(Command::Introduce(name)) = parse_one(phrase) else {
                panic!("expected Introduce for {phrase:?}");
            };
            assert_eq!(name, "Маша");
        }
    }

    #[test]
    fn introduction_with_unknown_name_goes_to_llm() {
        assert!(matches!(parse_one("это Вася"), Parsed::Ask { .. }));
    }

    #[test]
    fn recognizes_forget() {
        for phrase in ["забудь", "забудь всё", "сбрось контекст", "начни сначала"]
        {
            assert!(
                matches!(parse_one(phrase), Parsed::Command(Command::Forget)),
                "{phrase}"
            );
        }
    }

    #[test]
    fn recognizes_window_size() {
        let Parsed::Command(Command::SetWindow(n)) = parse_one("помни последние 5 реплик")
        else {
            panic!("expected SetWindow");
        };
        assert_eq!(n, 5);
    }

    #[test]
    fn recognizes_model_switching() {
        assert!(matches!(
            parse_one("переключись на умную модель"),
            Parsed::Command(Command::UseSmartModel)
        ));
        assert!(matches!(
            parse_one("смени модель на быструю"),
            Parsed::Command(Command::UseFastModel)
        ));
    }

    #[test]
    fn recognizes_stats_whoami_help_exit() {
        assert!(matches!(
            parse_one("сколько мы потратили"),
            Parsed::Command(Command::UsageStats)
        ));
        assert!(matches!(
            parse_one("кто я"),
            Parsed::Command(Command::WhoAmI)
        ));
        assert!(matches!(
            parse_one("помощь"),
            Parsed::Command(Command::Help)
        ));
        assert!(matches!(
            parse_one("выйди из режима"),
            Parsed::Command(Command::ExitMode)
        ));
    }

    #[test]
    fn recognizes_mode_triggers() {
        let Parsed::Command(Command::EnterMode(name)) = parse_one("Расскажи сказку про кота")
        else {
            panic!("expected EnterMode");
        };
        assert_eq!(name, "fairy_tale");
    }

    #[test]
    fn think_hard_prefix_upgrades_model() {
        let Parsed::Ask { text, think_hard } = parse_one("подумай как следует почему небо синее")
        else {
            panic!("expected Ask");
        };
        assert!(think_hard);
        assert_eq!(text, "почему небо синее");
    }

    #[test]
    fn plain_question_goes_to_llm() {
        let Parsed::Ask { text, think_hard } = parse_one("Почему трава зелёная?")
        else {
            panic!("expected Ask");
        };
        assert!(!think_hard);
        assert_eq!(text, "Почему трава зелёная?");
    }

    #[test]
    fn empty_utterance_maps_to_help() {
        assert!(matches!(parse_one("  "), Parsed::Command(Command::Help)));
    }
}
