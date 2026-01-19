use crate::fuzzy::fuzzy_match;

/// Wake word detection - checks if transcribed text starts with wake phrase
pub struct WakeWord {
    #[allow(dead_code)]
    phrase: String,
    words: Vec<String>,
}

impl WakeWord {
    pub fn new(phrase: &str) -> Self {
        Self {
            phrase: phrase.to_string(),
            words: phrase
                .to_lowercase()
                .split_whitespace()
                .map(String::from)
                .collect(),
        }
    }

    /// Check if text starts with wake word (fuzzy), return remaining text if matched
    pub fn detect(&self, text: &str) -> Option<String> {
        let text_words: Vec<&str> = text.split_whitespace().collect();
        if text_words.len() < self.words.len() {
            return None;
        }

        // Check each wake word with fuzzy matching
        for (i, wake_word) in self.words.iter().enumerate() {
            let spoken = text_words[i].to_lowercase();
            let spoken_clean: String = spoken.chars().filter(|c| c.is_alphabetic()).collect();
            if !fuzzy_match(wake_word, &spoken_clean) {
                return None;
            }
        }

        // Return the rest of the text after wake words
        let rest: String = text_words[self.words.len()..].join(" ");
        let rest = rest.trim_start_matches([',', '!', '.', ' ']).to_string();
        Some(rest)
    }

    #[allow(dead_code)]
    pub fn phrase(&self) -> &str {
        &self.phrase
    }
}
