//! REPL input handling - keyboard and voice input processing

use crate::render::Ui;
use crate::wake::WakeWord;
use std::time::{Duration, Instant};

/// Events from audio transcription pipeline
pub enum TranscriptEvent {
    Preview(String),
    Final(String),
}

/// Handle voice transcript event, returns text to add to input if any
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
