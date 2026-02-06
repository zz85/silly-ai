//! Main typing processor with undo support
//!
//! Processes transcribed segments, types text, and executes commands.
//! Maintains an undo buffer for reverting operations.

use super::commands::{CommandParser, TypingCommand};
use super::input::{InputMethod, TypingError, TypingInput};
use enigo::Key;
use std::collections::VecDeque;
use std::io::{self, Write};

/// Represents a typed operation for undo/redo
#[derive(Debug, Clone)]
enum TypedOperation {
    /// Typed text (stores the text for undo)
    Text(String),
    /// Typed punctuation
    Punctuation(char),
    /// Pressed Enter
    Enter,
}

/// Result of processing a segment
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessResult {
    /// Continue processing
    Continue,
    /// Stop command received - exit typing mode
    Stop,
    /// Pause command received - mute mic
    Pause,
    /// Resume command received - unmute mic
    Resume,
}

/// Main typing processor
pub struct TypingProcessor {
    input: TypingInput,
    parser: CommandParser,
    undo_stack: VecDeque<TypedOperation>,
    redo_stack: Vec<TypedOperation>,
    undo_buffer_size: usize,
    feedback_enabled: bool,
    verbose: bool,
    /// Track the last character typed for smart spacing
    last_char: Option<char>,
    /// Track if we need to capitalize the next word
    capitalize_next: bool,
}

impl TypingProcessor {
    /// Create a new typing processor
    pub fn new(
        method: InputMethod,
        undo_buffer_size: usize,
        feedback_enabled: bool,
        command_pause_ms: u32,
    ) -> Result<Self, TypingError> {
        Ok(Self {
            input: TypingInput::new(method)?,
            parser: CommandParser::new(command_pause_ms),
            undo_stack: VecDeque::with_capacity(undo_buffer_size),
            redo_stack: Vec::new(),
            undo_buffer_size,
            feedback_enabled,
            verbose: false,
            last_char: None,
            capitalize_next: true, // Start with capital
        })
    }

    /// Enable verbose logging
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Process a transcribed segment
    ///
    /// Returns the result indicating whether to continue, stop, pause, or resume
    pub fn process_segment(
        &mut self,
        text: &str,
        pause_ms: u32,
    ) -> Result<ProcessResult, TypingError> {
        if self.verbose {
            eprintln!("[TYPING] Input: \"{}\" (pause: {}ms)", text, pause_ms);
        }

        let result = self.parser.parse(text, pause_ms);

        if self.verbose {
            eprintln!(
                "[TYPING] Parsed: text={:?}, commands={:?}, had_command={}",
                result.text, result.commands, result.had_command
            );
        }

        // Type any text first
        if let Some(ref text) = result.text {
            // Smart spacing: add space before text if needed
            let text_to_type = self.prepare_text(text);

            if self.verbose {
                if text_to_type != *text {
                    eprintln!("[TYPING] Smart text: \"{}\" -> \"{}\"", text, text_to_type);
                } else {
                    eprintln!("[TYPING] Typing text: \"{}\"", text_to_type);
                }
            }
            self.type_text(&text_to_type)?;
        }

        // Execute commands
        for cmd in result.commands {
            if self.verbose {
                eprintln!("[TYPING] Executing command: {:?}", cmd);
            }
            match cmd {
                TypingCommand::Stop => {
                    self.play_feedback("stop");
                    return Ok(ProcessResult::Stop);
                }
                TypingCommand::Pause => {
                    self.play_feedback("pause");
                    return Ok(ProcessResult::Pause);
                }
                TypingCommand::Resume => {
                    self.play_feedback("resume");
                    return Ok(ProcessResult::Resume);
                }
                _ => {
                    self.execute_command(cmd)?;
                }
            }
        }

        // Provide feedback if a command was recognized
        if result.had_command {
            self.play_feedback("cmd");
        }

        Ok(ProcessResult::Continue)
    }

    /// Prepare text for typing with smart spacing and capitalization
    fn prepare_text(&mut self, text: &str) -> String {
        let mut result = String::new();

        // Check if we need a leading space
        let needs_space = self.last_char.map_or(false, |c| {
            // Add space if last char was alphanumeric or closing punctuation
            c.is_alphanumeric() || c == ')' || c == ']' || c == '}' || c == '"' || c == '\''
        });

        if needs_space && !text.is_empty() {
            let first = text.chars().next().unwrap();
            // Don't add space before punctuation
            if !first.is_ascii_punctuation() {
                result.push(' ');
            }
        }

        // Apply capitalization if needed
        let text_to_add = if self.capitalize_next && !text.is_empty() {
            let mut chars = text.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    s.push_str(chars.as_str());
                    s
                }
                None => text.to_string(),
            }
        } else {
            text.to_string()
        };

        result.push_str(&text_to_add);
        result
    }

    /// Type text and add to undo buffer
    fn type_text(&mut self, text: &str) -> Result<(), TypingError> {
        if text.is_empty() {
            return Ok(());
        }

        self.input.type_text(text)?;
        self.push_undo(TypedOperation::Text(text.to_string()));
        self.redo_stack.clear(); // Clear redo on new action

        // Update state tracking
        if let Some(c) = text.chars().last() {
            self.last_char = Some(c);
            // Capitalize after sentence-ending punctuation
            self.capitalize_next = c == '.' || c == '!' || c == '?';
        }

        Ok(())
    }

    /// Execute a typing command
    fn execute_command(&mut self, cmd: TypingCommand) -> Result<(), TypingError> {
        match cmd {
            TypingCommand::Undo => self.undo()?,
            TypingCommand::Redo => self.redo()?,

            TypingCommand::Punctuation(c) => {
                // Smart punctuation: add space after if it's sentence-ending
                let text = c.to_string();
                self.input.type_text(&text)?;
                self.push_undo(TypedOperation::Punctuation(c));
                self.redo_stack.clear();

                // Update state
                self.last_char = Some(c);
                self.capitalize_next = c == '.' || c == '!' || c == '?';
            }

            TypingCommand::Enter => {
                self.input.send_key(Key::Return)?;
                self.push_undo(TypedOperation::Enter);
                self.redo_stack.clear();

                // After enter, capitalize next and reset last_char
                self.last_char = Some('\n');
                self.capitalize_next = true;
            }

            TypingCommand::Tab => {
                self.input.send_key(Key::Tab)?;
                self.last_char = Some('\t');
            }

            TypingCommand::Space => {
                self.input.send_key(Key::Space)?;
                self.last_char = Some(' ');
            }

            TypingCommand::Backspace => {
                self.input.send_key(Key::Backspace)?;
                // After backspace, we don't know what char is now last
                // Reset to unknown state - next text will add space if needed
                self.last_char = None;
            }

            TypingCommand::Delete => {
                self.input.send_key(Key::Delete)?;
            }

            TypingCommand::DeleteWord => {
                // Option+Backspace on macOS, Ctrl+Backspace elsewhere
                #[cfg(target_os = "macos")]
                self.input.send_key_combo(&[Key::Alt], Key::Backspace)?;
                #[cfg(not(target_os = "macos"))]
                self.input.send_key_combo(&[Key::Control], Key::Backspace)?;
                self.last_char = None;
            }

            TypingCommand::DeleteLine => {
                // Select line then delete
                // Cmd+Shift+Left to select to start, then delete
                #[cfg(target_os = "macos")]
                {
                    self.input
                        .send_key_combo(&[Key::Meta, Key::Shift], Key::LeftArrow)?;
                    self.input.send_key(Key::Backspace)?;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    self.input.send_key(Key::Home)?;
                    self.input.send_key_combo(&[Key::Shift], Key::End)?;
                    self.input.send_key(Key::Backspace)?;
                }
                self.last_char = None;
                self.capitalize_next = true;
            }

            TypingCommand::SelectAll => {
                self.input
                    .send_key_combo(&[TypingInput::modifier_key()], Key::Unicode('a'))?;
            }

            TypingCommand::SelectWord => {
                // Double-click simulation is tricky, use Shift+Option+Left/Right on macOS
                #[cfg(target_os = "macos")]
                {
                    self.input
                        .send_key_combo(&[Key::Alt, Key::Shift], Key::LeftArrow)?;
                    self.input
                        .send_key_combo(&[Key::Alt, Key::Shift], Key::RightArrow)?;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    self.input
                        .send_key_combo(&[Key::Control, Key::Shift], Key::LeftArrow)?;
                }
            }

            TypingCommand::SelectLine => {
                #[cfg(target_os = "macos")]
                {
                    self.input.send_key_combo(&[Key::Meta], Key::LeftArrow)?;
                    self.input
                        .send_key_combo(&[Key::Meta, Key::Shift], Key::RightArrow)?;
                }
                #[cfg(not(target_os = "macos"))]
                {
                    self.input.send_key(Key::Home)?;
                    self.input.send_key_combo(&[Key::Shift], Key::End)?;
                }
            }

            TypingCommand::GoToEndOfLine => {
                #[cfg(target_os = "macos")]
                self.input.send_key_combo(&[Key::Meta], Key::RightArrow)?;
                #[cfg(not(target_os = "macos"))]
                self.input.send_key(Key::End)?;
            }

            TypingCommand::GoToStartOfLine => {
                #[cfg(target_os = "macos")]
                self.input.send_key_combo(&[Key::Meta], Key::LeftArrow)?;
                #[cfg(not(target_os = "macos"))]
                self.input.send_key(Key::Home)?;
            }

            TypingCommand::GoToEnd => {
                #[cfg(target_os = "macos")]
                self.input.send_key_combo(&[Key::Meta], Key::DownArrow)?;
                #[cfg(not(target_os = "macos"))]
                self.input.send_key_combo(&[Key::Control], Key::End)?;
            }

            TypingCommand::GoToStart => {
                #[cfg(target_os = "macos")]
                self.input.send_key_combo(&[Key::Meta], Key::UpArrow)?;
                #[cfg(not(target_os = "macos"))]
                self.input.send_key_combo(&[Key::Control], Key::Home)?;
            }

            TypingCommand::MoveLeft(n) => {
                for _ in 0..n {
                    self.input.send_key(Key::LeftArrow)?;
                }
            }

            TypingCommand::MoveRight(n) => {
                for _ in 0..n {
                    self.input.send_key(Key::RightArrow)?;
                }
            }

            TypingCommand::MoveUp(n) => {
                for _ in 0..n {
                    self.input.send_key(Key::UpArrow)?;
                }
            }

            TypingCommand::MoveDown(n) => {
                for _ in 0..n {
                    self.input.send_key(Key::DownArrow)?;
                }
            }

            // Control commands handled in process_segment
            TypingCommand::Stop | TypingCommand::Pause | TypingCommand::Resume => {}
        }

        Ok(())
    }

    /// Undo the last operation
    fn undo(&mut self) -> Result<(), TypingError> {
        if let Some(op) = self.undo_stack.pop_back() {
            match &op {
                TypedOperation::Text(text) => {
                    // Select and delete the text we typed
                    // This is a simple approach - select backwards by text length
                    for _ in 0..text.chars().count() {
                        self.input.send_key_combo(&[Key::Shift], Key::LeftArrow)?;
                    }
                    self.input.send_key(Key::Backspace)?;
                }
                TypedOperation::Punctuation(_) => {
                    self.input.send_key(Key::Backspace)?;
                }
                TypedOperation::Enter => {
                    self.input.send_key(Key::Backspace)?;
                }
            }
            self.redo_stack.push(op);
        }
        Ok(())
    }

    /// Redo the last undone operation
    fn redo(&mut self) -> Result<(), TypingError> {
        if let Some(op) = self.redo_stack.pop() {
            match &op {
                TypedOperation::Text(text) => {
                    self.input.type_text(text)?;
                }
                TypedOperation::Punctuation(c) => {
                    self.input.type_text(&c.to_string())?;
                }
                TypedOperation::Enter => {
                    self.input.send_key(Key::Return)?;
                }
            }
            self.push_undo(op);
        }
        Ok(())
    }

    /// Push an operation to the undo stack
    fn push_undo(&mut self, op: TypedOperation) {
        if self.undo_stack.len() >= self.undo_buffer_size {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(op);
    }

    /// Play feedback (visual/audio) when command is recognized
    fn play_feedback(&self, cmd_type: &str) {
        if !self.feedback_enabled {
            return;
        }

        match cmd_type {
            "stop" => {
                Self::beep();
                eprintln!("\n[Typing stopped - say 'silly type' to resume]");
            }
            "pause" => {
                Self::beep();
                eprintln!("\n[Typing paused - say 'silly type' to resume]");
            }
            "resume" => {
                Self::beep();
                eprintln!("\n[Typing resumed]");
            }
            "cmd" => {
                // Brief visual indicator that command was recognized
                // Don't beep for every command - too noisy
                if self.verbose {
                    eprint!("*");
                    let _ = io::stderr().flush();
                }
            }
            _ => {}
        }
    }

    /// Play a system beep sound
    fn beep() {
        // Terminal bell - works on most terminals
        eprint!("\x07");
        let _ = io::stderr().flush();

        // On macOS, we could also use the system sound
        #[cfg(target_os = "macos")]
        {
            // Try to play system sound asynchronously
            std::thread::spawn(|| {
                let _ = std::process::Command::new("afplay")
                    .arg("/System/Library/Sounds/Pop.aiff")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            });
        }
    }

    /// Get the number of operations in the undo buffer
    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    /// Get the number of operations in the redo buffer
    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}
