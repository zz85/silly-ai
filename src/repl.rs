//! REPL input handling - keyboard and voice input processing

use crate::command::{CommandProcessor, CommandResult};
use crate::render::Ui;
use crate::state::{AppMode, SharedState};
use crate::wake::WakeWord;
use std::time::{Duration, Instant};

/// Events from audio transcription pipeline
pub enum TranscriptEvent {
    Preview(String),
    Final(String),
}

/// Result of transcript handling
pub enum TranscriptResult {
    /// Text should be sent to LLM
    SendToLlm(String),
    /// Text should be transcribed only (no LLM)
    TranscribeOnly(String),
    /// Text should be appended to notes
    AppendNote(String),
    /// Command was handled (with optional response message)
    CommandHandled(Option<String>),
    /// Stop command (cancel TTS)
    Stop,
    /// Mode change command
    ModeChange {
        mode: AppMode,
        announcement: Option<String>,
    },
    /// Shutdown requested
    Shutdown,
    /// No action needed
    None,
}

/// Handle voice transcript event, returns text to add to input if any
#[allow(dead_code)]
pub fn handle_transcript(
    event: TranscriptEvent,
    wake_word: &WakeWord,
    last_interaction: Option<Instant>,
    wake_timeout: Duration,
    wake_enabled: bool,
    ui: &Ui,
) -> Option<String> {
    match event {
        TranscriptEvent::Preview(text) => {
            ui.set_preview(text);
            None
        }
        TranscriptEvent::Final(text) => {
            ui.set_idle(); // Clear preview

            let in_conversation = last_interaction
                .map(|t| t.elapsed() < wake_timeout)
                .unwrap_or(false);

            let command = if !wake_enabled || in_conversation {
                text
            } else {
                match wake_word.detect(&text) {
                    Some(cmd) => cmd,
                    None => return None,
                }
            };

            if command.is_empty() {
                return None;
            }

            Some(command)
        }
    }
}

/// Handle voice transcript event with mode awareness
///
/// This is the mode-aware version that handles different behaviors based on AppMode:
/// - Idle: Requires wake word, sends to LLM
/// - Chat: No wake word needed, sends to LLM
/// - Transcribe: STT only, no LLM processing
/// - NoteTaking: Append to notes file
/// - Command: Only processes commands, no LLM
pub fn handle_transcript_with_mode(
    event: TranscriptEvent,
    wake_word: &WakeWord,
    last_interaction: Option<Instant>,
    wake_timeout: Duration,
    state: &SharedState,
    command_processor: &CommandProcessor,
    ui: &Ui,
) -> TranscriptResult {
    let mode = state.mode();
    let wake_enabled = state.wake_enabled.load(std::sync::atomic::Ordering::SeqCst);
    
    match event {
        TranscriptEvent::Preview(text) => {
            ui.set_preview(text);
            TranscriptResult::None
        }
        TranscriptEvent::Final(text) => {
            ui.set_idle(); // Clear preview

            if text.is_empty() {
                return TranscriptResult::None;
            }

            // First, check if this is a command (in all modes except Transcribe/NoteTaking)
            let should_check_commands = !matches!(mode, AppMode::Transcribe | AppMode::NoteTaking);
            
            if should_check_commands {
                let cmd_result = command_processor.process(&text, state);
                match cmd_result {
                    CommandResult::Stop => return TranscriptResult::Stop,
                    CommandResult::Shutdown => return TranscriptResult::Shutdown,
                    CommandResult::Handled(msg) => return TranscriptResult::CommandHandled(msg),
                    CommandResult::ModeChange { mode, announcement } => {
                        return TranscriptResult::ModeChange { mode, announcement };
                    }
                    CommandResult::PassThrough(text) => {
                        // Not a command, continue with mode-specific handling
                        match mode {
                            AppMode::Idle => {
                                // Idle mode: requires wake word unless in conversation
                                let in_conversation = last_interaction
                                    .map(|t| t.elapsed() < wake_timeout)
                                    .unwrap_or(false);

                                let command = if !wake_enabled || in_conversation {
                                    text
                                } else {
                                    match wake_word.detect(&text) {
                                        Some(cmd) => cmd,
                                        None => return TranscriptResult::None,
                                    }
                                };

                                if command.is_empty() {
                                    return TranscriptResult::None;
                                }

                                TranscriptResult::SendToLlm(command)
                            }
                            AppMode::Chat => {
                                // Chat mode: no wake word needed, always send to LLM
                                state.update_last_interaction();
                                TranscriptResult::SendToLlm(text)
                            }
                            AppMode::Command => {
                                // Command mode: if not a command, show message
                                TranscriptResult::CommandHandled(Some(format!("[Not a command] {}", text)))
                            }
                            _ => TranscriptResult::None,
                        }
                    }
                }
            } else {
                // Transcribe and NoteTaking modes don't process commands
                match mode {
                    AppMode::Transcribe => TranscriptResult::TranscribeOnly(text),
                    AppMode::NoteTaking => TranscriptResult::AppendNote(text),
                    _ => TranscriptResult::None,
                }
            }
        }
    }
}

/// Append text to the notes file
pub fn append_to_notes(text: &str) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("notes.txt")?;
    
    // Add timestamp
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    writeln!(file, "[{}] {}", timestamp, text)?;
    
    Ok(())
}
