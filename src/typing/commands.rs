//! Voice command definitions and smart parsing
//!
//! Handles the distinction between text to type and commands to execute.
//! Uses smart detection based on pause duration, phrase length, and patterns.

use std::collections::HashMap;

/// Typing commands that can be recognized from speech
#[derive(Debug, Clone, PartialEq)]
pub enum TypingCommand {
    // Punctuation - insert character
    Punctuation(char),

    // Keys
    Enter,
    Tab,
    Space,
    Backspace,
    Delete,

    // Word-level editing
    DeleteWord,
    DeleteLine,

    // Undo/Redo
    Undo,
    Redo,

    // Selection
    SelectAll,
    SelectWord,
    SelectLine,

    // Navigation
    GoToEndOfLine,
    GoToStartOfLine,
    GoToEnd,
    GoToStart,
    MoveLeft(u32),
    MoveRight(u32),
    MoveUp(u32),
    MoveDown(u32),

    // Control
    Stop,   // Exit typing mode
    Pause,  // Pause (mute mic)
    Resume, // Resume from pause
}

/// Result of parsing a transcribed segment
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// Text to type (if any)
    pub text: Option<String>,
    /// Commands to execute (in order)
    pub commands: Vec<TypingCommand>,
    /// Whether a command was recognized (for feedback)
    pub had_command: bool,
}

impl ParseResult {
    pub fn text_only(text: String) -> Self {
        Self {
            text: Some(text),
            commands: vec![],
            had_command: false,
        }
    }

    pub fn command_only(cmd: TypingCommand) -> Self {
        Self {
            text: None,
            commands: vec![cmd],
            had_command: true,
        }
    }

    pub fn text_and_commands(text: String, commands: Vec<TypingCommand>) -> Self {
        let had_command = !commands.is_empty();
        Self {
            text: Some(text),
            commands,
            had_command,
        }
    }

    pub fn empty() -> Self {
        Self {
            text: None,
            commands: vec![],
            had_command: false,
        }
    }
}

/// Parser for detecting commands in transcribed speech
pub struct CommandParser {
    /// Phrase -> Command mappings (lowercase)
    patterns: HashMap<String, TypingCommand>,
    /// Punctuation phrase -> char mappings (lowercase)
    punctuation: HashMap<String, char>,
    /// Minimum pause duration (ms) to consider short phrase as pure command
    min_pause_for_command: u32,
    /// Maximum words for a "short phrase" that could be a pure command
    max_words_for_command: usize,
}

impl Default for CommandParser {
    fn default() -> Self {
        Self::new(100) // Default 100ms
    }
}

impl CommandParser {
    /// Create a new command parser with default patterns
    pub fn new(min_pause_for_command: u32) -> Self {
        let mut patterns = HashMap::new();
        let mut punctuation = HashMap::new();

        // Enter/Return commands
        for phrase in &["enter", "return", "new line", "newline"] {
            patterns.insert(phrase.to_string(), TypingCommand::Enter);
        }
        patterns.insert("new paragraph".to_string(), TypingCommand::Enter);
        patterns.insert("tab".to_string(), TypingCommand::Tab);
        patterns.insert("space".to_string(), TypingCommand::Space);
        patterns.insert("spacebar".to_string(), TypingCommand::Space);

        // Editing commands
        patterns.insert("undo".to_string(), TypingCommand::Undo);
        patterns.insert("redo".to_string(), TypingCommand::Redo);
        patterns.insert("delete".to_string(), TypingCommand::Backspace);
        patterns.insert("backspace".to_string(), TypingCommand::Backspace);
        patterns.insert("back space".to_string(), TypingCommand::Backspace);
        patterns.insert("delete word".to_string(), TypingCommand::DeleteWord);
        patterns.insert("delete line".to_string(), TypingCommand::DeleteLine);
        patterns.insert("clear line".to_string(), TypingCommand::DeleteLine);

        // Selection commands
        patterns.insert("select all".to_string(), TypingCommand::SelectAll);
        patterns.insert("select word".to_string(), TypingCommand::SelectWord);
        patterns.insert("select line".to_string(), TypingCommand::SelectLine);

        // Navigation commands
        for phrase in &[
            "go to end of line",
            "go to end of the line",
            "end of line",
            "go to end",
        ] {
            patterns.insert(phrase.to_string(), TypingCommand::GoToEndOfLine);
        }
        for phrase in &[
            "go to start of line",
            "go to start of the line",
            "go to beginning of line",
            "go to beginning of the line",
            "start of line",
            "beginning of line",
            "go to start",
            "go to beginning",
        ] {
            patterns.insert(phrase.to_string(), TypingCommand::GoToStartOfLine);
        }

        // Control commands - require "silly" prefix OR postfix to avoid accidental triggers
        // Exit commands: "silly terminate", "silly end", etc.
        for word in &["terminate", "end", "quit", "exit", "close"] {
            patterns.insert(format!("silly {}", word), TypingCommand::Stop);
            patterns.insert(format!("{} silly", word), TypingCommand::Stop);
        }

        // Pause commands: "silly stop", "silly pause", etc.
        for word in &["stop", "pause", "mute", "off", "hold"] {
            patterns.insert(format!("silly {}", word), TypingCommand::Pause);
            patterns.insert(format!("{} silly", word), TypingCommand::Pause);
        }

        // Resume commands: "silly type", "silly start", etc.
        for word in &["type", "start", "resume", "on", "unmute", "go", "continue"] {
            patterns.insert(format!("silly {}", word), TypingCommand::Resume);
            patterns.insert(format!("{} silly", word), TypingCommand::Resume);
        }

        // Also add "silly toggle" for pause
        patterns.insert("silly toggle".to_string(), TypingCommand::Pause);
        patterns.insert("toggle silly".to_string(), TypingCommand::Pause);

        // Punctuation mappings
        punctuation.insert("period".to_string(), '.');
        punctuation.insert("dot".to_string(), '.');
        punctuation.insert("full stop".to_string(), '.');
        punctuation.insert("comma".to_string(), ',');
        punctuation.insert("question mark".to_string(), '?');
        punctuation.insert("exclamation point".to_string(), '!');
        punctuation.insert("exclamation mark".to_string(), '!');
        punctuation.insert("colon".to_string(), ':');
        punctuation.insert("semicolon".to_string(), ';');
        punctuation.insert("dash".to_string(), '-');
        punctuation.insert("hyphen".to_string(), '-');
        punctuation.insert("open parenthesis".to_string(), '(');
        punctuation.insert("close parenthesis".to_string(), ')');
        punctuation.insert("open paren".to_string(), '(');
        punctuation.insert("close paren".to_string(), ')');
        punctuation.insert("open bracket".to_string(), '[');
        punctuation.insert("close bracket".to_string(), ']');
        punctuation.insert("open brace".to_string(), '{');
        punctuation.insert("close brace".to_string(), '}');
        punctuation.insert("open quote".to_string(), '"');
        punctuation.insert("close quote".to_string(), '"');
        punctuation.insert("quote".to_string(), '"');
        punctuation.insert("double quote".to_string(), '"');
        punctuation.insert("single quote".to_string(), '\'');
        punctuation.insert("apostrophe".to_string(), '\'');
        punctuation.insert("at sign".to_string(), '@');
        punctuation.insert("at".to_string(), '@');
        punctuation.insert("hashtag".to_string(), '#');
        punctuation.insert("hash".to_string(), '#');
        punctuation.insert("pound".to_string(), '#');
        punctuation.insert("dollar sign".to_string(), '$');
        punctuation.insert("dollar".to_string(), '$');
        punctuation.insert("percent".to_string(), '%');
        punctuation.insert("percent sign".to_string(), '%');
        punctuation.insert("ampersand".to_string(), '&');
        punctuation.insert("and sign".to_string(), '&');
        punctuation.insert("asterisk".to_string(), '*');
        punctuation.insert("star".to_string(), '*');
        punctuation.insert("underscore".to_string(), '_');
        punctuation.insert("plus".to_string(), '+');
        punctuation.insert("plus sign".to_string(), '+');
        punctuation.insert("equals".to_string(), '=');
        punctuation.insert("equals sign".to_string(), '=');
        punctuation.insert("slash".to_string(), '/');
        punctuation.insert("forward slash".to_string(), '/');
        punctuation.insert("backslash".to_string(), '\\');
        punctuation.insert("back slash".to_string(), '\\');
        punctuation.insert("pipe".to_string(), '|');
        punctuation.insert("tilde".to_string(), '~');
        punctuation.insert("caret".to_string(), '^');
        punctuation.insert("less than".to_string(), '<');
        punctuation.insert("greater than".to_string(), '>');

        Self {
            patterns,
            punctuation,
            min_pause_for_command,
            max_words_for_command: 4, // Commands are typically short
        }
    }

    /// Print all available voice commands
    pub fn print_help() {
        eprintln!(
            "
╭─────────────────────────────────────────────────────────────╮
│                  VOICE COMMANDS                             │
├─────────────────────────────────────────────────────────────┤
│ CONTROL (use 'Silly' as prefix or postfix)                  │
│   silly terminate / terminate silly   Exit typing mode      │
│   silly end / end silly               Exit typing mode      │
│   silly quit / quit silly             Exit typing mode      │
│   silly stop / stop silly             Pause typing          │
│   silly pause / pause silly           Pause typing          │
│   silly mute / mute silly             Pause typing          │
│   silly type / type silly             Resume typing         │
│   silly start / start silly           Resume typing         │
│   silly resume / resume silly         Resume typing         │
├─────────────────────────────────────────────────────────────┤
│ PUNCTUATION                                                 │
│   period / dot                     .                        │
│   comma                            ,                        │
│   question mark                    ?                        │
│   exclamation point / mark         !                        │
│   colon / semicolon                : / ;                    │
│   dash / hyphen                    -                        │
│   quote / double quote             \"                        │
│   apostrophe / single quote        '                        │
│   open/close parenthesis           ( )                      │
│   open/close bracket               [ ]                      │
│   at sign / hashtag                @ #                      │
├─────────────────────────────────────────────────────────────┤
│ KEYS                                                        │
│   enter / return / new line        ⏎                        │
│   tab                              ⇥                        │
│   space / spacebar                 ␣                        │
│   back space / backspace / delete  ⌫                        │
├─────────────────────────────────────────────────────────────┤
│ EDITING                                                     │
│   undo / redo                      Undo/redo last action    │
│   delete word / delete line        Delete word/line         │
│   select all / select word         Selection                │
├─────────────────────────────────────────────────────────────┤
│ NAVIGATION                                                  │
│   go to end of line                Move to end of line      │
│   go to start of line              Move to start of line    │
├─────────────────────────────────────────────────────────────┤
│ HOTKEYS                                                     │
│   Double-tap Cmd                   Toggle typing on/off     │
│   Ctrl+Space                       Push-to-talk             │
╰─────────────────────────────────────────────────────────────╯
"
        );
    }

    /// Smart parse: considers pause duration and segment characteristics
    ///
    /// Logic:
    /// 1. If segment is short (1-4 words) AND matches a command pattern -> pure command
    /// 2. If segment ends with command phrase -> text + command
    /// 3. Otherwise -> pure text (replace spoken punctuation inline)
    pub fn parse(&self, text: &str, pause_duration_ms: u32) -> ParseResult {
        let text = text.trim();
        if text.is_empty() {
            return ParseResult::empty();
        }

        // Normalize: lowercase and strip trailing punctuation for command matching
        let lower = text.to_lowercase();
        let normalized = lower.trim_end_matches(|c: char| c.is_ascii_punctuation());

        // Step 1: Check if it's a pure command (short phrase after pause)
        if let Some(cmd) = self.is_pure_command(normalized, pause_duration_ms) {
            return ParseResult::command_only(cmd);
        }

        // Step 2: Extract trailing commands and process remaining text
        let (processed_text, commands) = self.extract_trailing_commands(normalized);

        // Step 3: Replace inline punctuation in the remaining text
        let final_text = self.replace_inline_punctuation(&processed_text);

        if final_text.is_empty() && commands.is_empty() {
            ParseResult::empty()
        } else if final_text.is_empty() {
            ParseResult {
                text: None,
                commands,
                had_command: true,
            }
        } else {
            ParseResult::text_and_commands(final_text, commands)
        }
    }

    /// Check if text is a pure command (short phrase matching pattern)
    fn is_pure_command(&self, text: &str, pause_ms: u32) -> Option<TypingCommand> {
        let word_count = text.split_whitespace().count();

        // Single-word commands are more likely to be commands, lower threshold
        let pause_threshold = if word_count == 1 {
            100 // Lower threshold for single words
        } else {
            self.min_pause_for_command
        };

        // Short phrases after a pause are likely commands
        if word_count <= self.max_words_for_command && pause_ms >= pause_threshold {
            // Check exact command match
            if let Some(cmd) = self.patterns.get(text) {
                return Some(cmd.clone());
            }

            // Check punctuation
            if let Some(&c) = self.punctuation.get(text) {
                return Some(TypingCommand::Punctuation(c));
            }
        }

        None
    }

    /// Extract trailing commands from text
    /// Returns (remaining_text, commands)
    fn extract_trailing_commands(&self, text: &str) -> (String, Vec<TypingCommand>) {
        let mut commands = Vec::new();
        let mut remaining = text.to_string();

        // Keep extracting trailing commands until none found
        loop {
            let mut found = false;

            // Check for trailing command patterns (longest first)
            let mut sorted_patterns: Vec<_> = self.patterns.keys().collect();
            sorted_patterns.sort_by(|a, b| b.len().cmp(&a.len()));

            for pattern in sorted_patterns {
                if remaining.ends_with(pattern.as_str()) {
                    // Make sure it's a word boundary
                    let prefix_len = remaining.len() - pattern.len();
                    if prefix_len == 0
                        || remaining
                            .chars()
                            .nth(prefix_len - 1)
                            .map(|c| c.is_whitespace())
                            .unwrap_or(false)
                    {
                        commands.insert(0, self.patterns[pattern].clone());
                        remaining = remaining[..prefix_len].trim_end().to_string();
                        found = true;
                        break;
                    }
                }
            }

            // Check for trailing punctuation
            if !found {
                let mut sorted_punct: Vec<_> = self.punctuation.keys().collect();
                sorted_punct.sort_by(|a, b| b.len().cmp(&a.len()));

                for pattern in sorted_punct {
                    if remaining.ends_with(pattern.as_str()) {
                        let prefix_len = remaining.len() - pattern.len();
                        if prefix_len == 0
                            || remaining
                                .chars()
                                .nth(prefix_len - 1)
                                .map(|c| c.is_whitespace())
                                .unwrap_or(false)
                        {
                            commands
                                .insert(0, TypingCommand::Punctuation(self.punctuation[pattern]));
                            remaining = remaining[..prefix_len].trim_end().to_string();
                            found = true;
                            break;
                        }
                    }
                }
            }

            if !found {
                break;
            }
        }

        (remaining, commands)
    }

    /// Replace inline punctuation words with characters
    /// "hello comma world" -> "hello, world"
    fn replace_inline_punctuation(&self, text: &str) -> String {
        let mut result = text.to_string();

        // Sort by length (longest first) to avoid partial replacements
        let mut sorted_punct: Vec<_> = self.punctuation.iter().collect();
        sorted_punct.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        for (phrase, &c) in sorted_punct {
            // Replace " phrase " with "c " (space after punctuation)
            let pattern_with_spaces = format!(" {} ", phrase);
            let replacement = format!("{} ", c);
            result = result.replace(&pattern_with_spaces, &replacement);

            // Also handle " phrase" at end (but don't remove trailing since we want space handling)
        }

        // Clean up any double spaces
        while result.contains("  ") {
            result = result.replace("  ", " ");
        }

        result.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pure_command_after_pause() {
        let parser = CommandParser::default();

        // "enter" after 500ms pause should be a command
        let result = parser.parse("enter", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Enter]);

        // "undo" after pause
        let result = parser.parse("undo", 400);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Undo]);
    }

    #[test]
    fn test_text_with_trailing_command() {
        let parser = CommandParser::default();

        // Text ending with "enter"
        let result = parser.parse("hello world enter", 100);
        assert_eq!(result.text, Some("hello world".to_string()));
        assert_eq!(result.commands, vec![TypingCommand::Enter]);
    }

    #[test]
    fn test_inline_punctuation() {
        let parser = CommandParser::default();

        // "hello comma world" -> "hello, world"
        let result = parser.parse("hello comma world", 100);
        assert_eq!(result.text, Some("hello, world".to_string()));
        assert!(result.commands.is_empty());
    }

    #[test]
    fn test_text_with_trailing_punctuation() {
        let parser = CommandParser::default();

        // "hello world period"
        let result = parser.parse("hello world period", 100);
        assert_eq!(result.text, Some("hello world".to_string()));
        assert_eq!(result.commands, vec![TypingCommand::Punctuation('.')]);
    }

    #[test]
    fn test_control_commands_require_silly() {
        let parser = CommandParser::default();

        // Bare "stop" should NOT be a command - it should be typed as text
        let result = parser.parse("stop", 500);
        assert_eq!(result.text, Some("stop".to_string()));
        assert!(result.commands.is_empty());

        // Bare "terminate" should NOT be a command
        let result = parser.parse("terminate", 500);
        assert_eq!(result.text, Some("terminate".to_string()));
        assert!(result.commands.is_empty());

        // "silly stop" SHOULD be a Pause command
        let result = parser.parse("silly stop", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Pause]);

        // "stop silly" SHOULD also be a Pause command
        let result = parser.parse("stop silly", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Pause]);

        // "silly terminate" SHOULD be a Stop (exit) command
        let result = parser.parse("silly terminate", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Stop]);

        // "terminate silly" SHOULD also be a Stop (exit) command
        let result = parser.parse("terminate silly", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Stop]);

        // "silly type" SHOULD be a Resume command
        let result = parser.parse("silly type", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Resume]);
    }

    #[test]
    fn test_complex_sentence() {
        let parser = CommandParser::default();

        // Complex: "hello comma how are you question mark enter"
        let result = parser.parse("hello comma how are you question mark enter", 100);
        assert_eq!(result.text, Some("hello, how are you".to_string()));
        assert_eq!(
            result.commands,
            vec![TypingCommand::Punctuation('?'), TypingCommand::Enter]
        );
    }

    #[test]
    fn test_backspace_variations() {
        let parser = CommandParser::default();

        // "backspace" should be recognized
        let result = parser.parse("backspace", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Backspace]);

        // "back space" (two words) should also work
        let result = parser.parse("back space", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Backspace]);

        // "delete" should also trigger backspace
        let result = parser.parse("delete", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Backspace]);
    }

    #[test]
    fn test_navigation_commands() {
        let parser = CommandParser::default();

        // "go to end of line"
        let result = parser.parse("go to end of line", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::GoToEndOfLine]);

        // "go to start of line"
        let result = parser.parse("go to start of line", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::GoToStartOfLine]);
    }

    #[test]
    fn test_editing_commands() {
        let parser = CommandParser::default();

        // "select all"
        let result = parser.parse("select all", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::SelectAll]);

        // "undo"
        let result = parser.parse("undo", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Undo]);

        // "redo"
        let result = parser.parse("redo", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Redo]);

        // "delete word"
        let result = parser.parse("delete word", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::DeleteWord]);
    }

    #[test]
    fn test_punctuation_commands() {
        let parser = CommandParser::default();

        // Test various punctuation
        let tests = vec![
            ("period", '.'),
            ("dot", '.'),
            ("comma", ','),
            ("question mark", '?'),
            ("exclamation point", '!'),
            ("colon", ':'),
            ("semicolon", ';'),
            ("dash", '-'),
            ("hyphen", '-'),
            ("at sign", '@'),
            ("hashtag", '#'),
        ];

        for (phrase, expected_char) in tests {
            let result = parser.parse(phrase, 500);
            assert!(
                result.text.is_none(),
                "Expected no text for '{}', got {:?}",
                phrase,
                result.text
            );
            assert_eq!(
                result.commands,
                vec![TypingCommand::Punctuation(expected_char)],
                "Failed for '{}'",
                phrase
            );
        }
    }

    #[test]
    fn test_text_without_commands() {
        let parser = CommandParser::default();

        // Regular text without any commands
        let result = parser.parse("hello world", 100);
        assert_eq!(result.text, Some("hello world".to_string()));
        assert!(result.commands.is_empty());
        assert!(!result.had_command);
    }

    #[test]
    fn test_silly_in_regular_text() {
        let parser = CommandParser::default();

        // "silly" in the middle of text should NOT trigger control commands
        let result = parser.parse("that's a silly idea", 100);
        assert_eq!(result.text, Some("that's a silly idea".to_string()));
        assert!(result.commands.is_empty());

        // "silly" at start but followed by non-command word
        let result = parser.parse("silly rabbit", 100);
        assert_eq!(result.text, Some("silly rabbit".to_string()));
        assert!(result.commands.is_empty());
    }

    #[test]
    fn test_trailing_punctuation_stripped() {
        let parser = CommandParser::default();

        // Transcriber often adds periods - should be stripped for command matching
        let result = parser.parse("silly stop.", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Pause]);

        // Same for other punctuation
        let result = parser.parse("enter!", 500);
        assert!(result.text.is_none());
        assert_eq!(result.commands, vec![TypingCommand::Enter]);
    }

    #[test]
    fn test_multiple_inline_punctuation() {
        let parser = CommandParser::default();

        // Multiple inline punctuation (note: parser lowercases text)
        let result = parser.parse("hello comma I said comma world", 100);
        assert_eq!(result.text, Some("hello, i said, world".to_string()));
        assert!(result.commands.is_empty());
    }
}
