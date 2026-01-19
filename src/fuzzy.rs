//! Fuzzy string matching utilities using Levenshtein distance
//!
//! Provides fuzzy matching for wake words and commands to handle
//! transcription errors and variations in speech.

/// Fuzzy match using Levenshtein distance, allows ~30% errors
pub fn fuzzy_match(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }
    let max_dist = (expected.len() / 3).max(1);
    levenshtein(expected, actual) <= max_dist
}

/// Calculate Levenshtein distance between two strings
pub fn levenshtein(a: &str, b: &str) -> usize {
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

/// Clean text for matching: lowercase and remove non-alphabetic characters
pub fn clean_for_matching(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_whitespace())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(fuzzy_match("hello", "hello"));
        assert!(fuzzy_match("stop", "stop"));
    }

    #[test]
    fn test_fuzzy_match() {
        // Allow ~30% errors
        assert!(fuzzy_match("hello", "helo")); // 1 char off in 5-char word
        assert!(fuzzy_match("stop", "stpo")); // 1 char off in 4-char word
        assert!(fuzzy_match("assistant", "asistant")); // 1 char off in 9-char word
    }

    #[test]
    fn test_no_match() {
        assert!(!fuzzy_match("hello", "world"));
        assert!(!fuzzy_match("stop", "start"));
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("hello", "hello"), 0);
        assert_eq!(levenshtein("hello", "helo"), 1);
        assert_eq!(levenshtein("hello", "world"), 4);
    }

    #[test]
    fn test_clean_for_matching() {
        assert_eq!(clean_for_matching("Hello!"), "hello");
        assert_eq!(clean_for_matching("Stop."), "stop");
        assert_eq!(clean_for_matching("Hey, there!"), "hey there");
    }
}
