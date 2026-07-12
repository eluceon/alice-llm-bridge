/// A themed conversation preset (fairy tale, quiz, homework help, ...)
/// activated by voice and expressed as an extra system prompt block.
#[derive(Debug, Clone)]
pub struct Mode {
    pub name: String,
    /// Spoken phrases that switch the mode on.
    pub triggers: Vec<String>,
    pub prompt: String,
}
