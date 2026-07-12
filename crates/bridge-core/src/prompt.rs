//! Assembly of the system prompt from profile, family, mode and memory.

use std::fmt::Write;

use chrono::NaiveDate;

use crate::{FamilyRoster, Mode, Profile, ProfileRole};

/// Everything the prompt builder needs to know about the current turn.
pub struct PromptContext<'a> {
    pub today: NaiveDate,
    pub profile: &'a Profile,
    pub roster: &'a FamilyRoster,
    pub mode: Option<&'a Mode>,
    pub summary: Option<&'a str>,
}

/// Builds the system prompt sent with every model request.
pub fn build_system_prompt(ctx: &PromptContext) -> String {
    let mut prompt = String::from(
        "Ты — голосовой помощник семьи, отвечаешь через умную колонку. \
         Отвечай кратко, одним-тремя предложениями, без списков, без markdown \
         и без эмодзи: текст будет озвучен вслух.\n",
    );
    let _ = writeln!(prompt, "Сегодня {}.", ctx.today);

    let speaker = ctx.profile;
    match speaker.age_on(ctx.today) {
        Some(age) => {
            let _ = writeln!(
                prompt,
                "Сейчас с тобой говорит {} ({} лет). {}",
                speaker.name, age, speaker.persona
            );
        }
        None => {
            let _ = writeln!(
                prompt,
                "Сейчас с тобой говорит {}. {}",
                speaker.name, speaker.persona
            );
        }
    }
    if speaker.role == ProfileRole::Child {
        prompt.push_str(
            "Собеседник — ребенок: отвечай бережно, простыми словами, \
             без взрослых и пугающих тем.\n",
        );
    }

    let family: Vec<String> = ctx
        .roster
        .all()
        .iter()
        .map(|p| match p.birthday {
            Some(birthday) => format!("{} (день рождения {})", p.name, birthday),
            None => p.name.clone(),
        })
        .collect();
    let _ = writeln!(prompt, "Семья: {}.", family.join(", "));

    if let Some(mode) = ctx.mode {
        let _ = writeln!(prompt, "Текущий режим: {}", mode.prompt);
    }
    if let Some(summary) = ctx.summary {
        let _ = writeln!(prompt, "Краткая память о прошлых разговорах: {summary}");
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FamilyRoster, Mode, Profile, ProfileRole};
    use chrono::NaiveDate;

    fn roster() -> FamilyRoster {
        let profiles = vec![
            Profile {
                name: "Дима".to_string(),
                aliases: vec!["дима".to_string()],
                birthday: NaiveDate::from_ymd_opt(1985, 3, 10),
                role: ProfileRole::Adult,
                persona: "Общайся на равных.".to_string(),
            },
            Profile {
                name: "Маша".to_string(),
                aliases: vec!["маша".to_string()],
                birthday: NaiveDate::from_ymd_opt(2016, 9, 2),
                role: ProfileRole::Child,
                persona: "Дочь Маша.".to_string(),
            },
        ];
        FamilyRoster::new(profiles, "Дима").unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 11).unwrap()
    }

    #[test]
    fn adult_prompt_contains_basics() {
        let roster = roster();
        let ctx = PromptContext {
            today: today(),
            profile: roster.get("Дима").unwrap(),
            roster: &roster,
            mode: None,
            summary: None,
        };
        let prompt = build_system_prompt(&ctx);
        assert!(prompt.contains("голосовой помощник"));
        assert!(prompt.contains("Сегодня 2026-07-11"));
        assert!(prompt.contains("Дима"));
        assert!(prompt.contains("41"));
        assert!(prompt.contains("Общайся на равных."));
        assert!(prompt.contains("Маша (день рождения 2016-09-02)"));
        assert!(!prompt.contains("ребен"));
    }

    #[test]
    fn child_prompt_adds_safety_block() {
        let roster = roster();
        let ctx = PromptContext {
            today: today(),
            profile: roster.get("Маша").unwrap(),
            roster: &roster,
            mode: None,
            summary: None,
        };
        let prompt = build_system_prompt(&ctx);
        assert!(prompt.contains("ребенок") || prompt.contains("ребёнок"));
        assert!(prompt.contains("9"));
    }

    #[test]
    fn mode_and_summary_are_appended() {
        let roster = roster();
        let mode = Mode {
            name: "fairy_tale".to_string(),
            triggers: vec![],
            prompt: "Рассказывай сказки.".to_string(),
        };
        let ctx = PromptContext {
            today: today(),
            profile: roster.get("Дима").unwrap(),
            roster: &roster,
            mode: Some(&mode),
            summary: Some("Обсуждали Марс."),
        };
        let prompt = build_system_prompt(&ctx);
        assert!(prompt.contains("Рассказывай сказки."));
        assert!(prompt.contains("Обсуждали Марс."));
    }
}
