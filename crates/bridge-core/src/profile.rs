use chrono::NaiveDate;

use crate::error::{CoreError, Result};

/// Governs the tone and safety constraints applied to a family member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileRole {
    Adult,
    Child,
}

/// A family member the skill can talk to.
#[derive(Debug, Clone)]
pub struct Profile {
    /// Display name, also the storage key for the member's history.
    pub name: String,
    /// Lowercased spoken variants used to match introductions.
    pub aliases: Vec<String>,
    pub birthday: Option<NaiveDate>,
    pub role: ProfileRole,
    /// Free-form instructions appended to the system prompt.
    pub persona: String,
}

impl Profile {
    /// Completed years on `today`, if the birthday is known.
    pub fn age_on(&self, today: NaiveDate) -> Option<u32> {
        self.birthday
            .and_then(|birthday| today.years_since(birthday))
    }
}

/// All configured family members plus the profile used before anyone
/// introduces themselves.
#[derive(Debug, Clone)]
pub struct FamilyRoster {
    profiles: Vec<Profile>,
    default_index: usize,
}

impl FamilyRoster {
    pub fn new(profiles: Vec<Profile>, default_profile: &str) -> Result<Self> {
        let default_index = profiles
            .iter()
            .position(|p| p.name == default_profile)
            .ok_or_else(|| CoreError::UnknownProfile(default_profile.to_string()))?;
        Ok(Self {
            profiles,
            default_index,
        })
    }

    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// Matches a spoken, already-normalized name or alias.
    pub fn find_by_alias(&self, spoken: &str) -> Option<&Profile> {
        let spoken = spoken.trim();
        self.profiles
            .iter()
            .find(|p| p.name.to_lowercase() == spoken || p.aliases.iter().any(|a| a == spoken))
    }

    pub fn default_profile(&self) -> &Profile {
        &self.profiles[self.default_index]
    }

    pub fn all(&self) -> &[Profile] {
        &self.profiles
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn masha() -> Profile {
        Profile {
            name: "Маша".to_string(),
            aliases: vec!["маша".to_string(), "дочь".to_string()],
            birthday: NaiveDate::from_ymd_opt(2016, 9, 2),
            role: ProfileRole::Child,
            persona: "Дочь Маша.".to_string(),
        }
    }

    fn dima() -> Profile {
        Profile {
            name: "Дима".to_string(),
            aliases: vec!["дима".to_string(), "папа".to_string()],
            birthday: NaiveDate::from_ymd_opt(1985, 3, 10),
            role: ProfileRole::Adult,
            persona: String::new(),
        }
    }

    #[test]
    fn age_counts_completed_years() {
        let p = masha();
        let before = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let after = NaiveDate::from_ymd_opt(2026, 9, 2).unwrap();
        assert_eq!(p.age_on(before), Some(9));
        assert_eq!(p.age_on(after), Some(10));
    }

    #[test]
    fn age_is_none_without_birthday() {
        let mut p = masha();
        p.birthday = None;
        assert_eq!(p.age_on(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()), None);
    }

    #[test]
    fn roster_finds_by_alias_and_name() {
        let roster = FamilyRoster::new(vec![dima(), masha()], "Дима").unwrap();
        assert_eq!(roster.find_by_alias("дочь").unwrap().name, "Маша");
        assert_eq!(roster.find_by_alias("маша").unwrap().name, "Маша");
        assert_eq!(roster.find_by_alias("дима").unwrap().name, "Дима");
        assert!(roster.find_by_alias("незнакомец").is_none());
        assert_eq!(roster.get("Маша").unwrap().name, "Маша");
        assert_eq!(roster.default_profile().name, "Дима");
    }

    #[test]
    fn roster_rejects_unknown_default() {
        let err = FamilyRoster::new(vec![dima()], "Вася").unwrap_err();
        assert!(matches!(err, CoreError::UnknownProfile(_)));
    }
}
