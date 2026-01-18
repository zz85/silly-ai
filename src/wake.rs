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

/// Fuzzy match using Levenshtein distance, allows ~30% errors
fn fuzzy_match(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }
    let max_dist = (expected.len() / 3).max(1);
    levenshtein(expected, actual) <= max_dist
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0; b.len() + 1]; a.len() + 1];

    for i in 0..=a.len() {
        dp[i][0] = i;
    }
    for j in 0..=b.len() {
        dp[0][j] = j;
    }

    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[a.len()][b.len()]
}
