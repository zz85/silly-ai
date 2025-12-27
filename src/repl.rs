//! REPL input handling - keyboard and voice input processing

use crate::chat::Chat;
use crate::render::Ui;
use crate::tts::Tts;
use crate::wake::WakeWord;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Events from audio transcription pipeline
pub enum TranscriptEvent {
    Preview(String),
    Final(String),
}

/// Process a command: send to LLM, stream response, return sink for playback
pub async fn process_command(
    command: &str,
    tts_playing: &Arc<AtomicBool>,
    tts_engine: &Tts,
    chat: &mut Chat,
    ui: &Ui,
) -> Option<(rodio::OutputStream, rodio::Sink)> {
    tts_playing.store(true, Ordering::SeqCst);
    ui.set_thinking();

    match Tts::create_sink() {
        Ok((stream, sink)) => {
            let ui_resp = ui.clone();
            if let Err(e) = chat
                .send_streaming_with_callback(
                    command,
                    |sentence| {
                        let _ = tts_engine.queue(sentence, &sink);
                    },
                    |chunk| ui_resp.append_response(chunk),
                )
                .await
            {
                eprintln!("Chat error: {}", e);
            }
            ui.end_response();
            ui.set_context_words(chat.context_words());
            ui.set_speaking();
            Some((stream, sink))
        }
        Err(e) => {
            eprintln!("Audio error: {}", e);
            let ui_resp = ui.clone();
            let _ = chat
                .send_streaming_with_callback(command, |_| {}, |chunk| ui_resp.append_response(chunk))
                .await;
            ui.end_response();
            ui.set_context_words(chat.context_words());
            tts_playing.store(false, Ordering::SeqCst);
            None
        }
    }
}

/// Handle voice transcript event, returns text to add to input if any
pub fn handle_transcript(
    event: TranscriptEvent,
    wake_word: &WakeWord,
    last_interaction: Option<Instant>,
    wake_timeout: Duration,
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

            let command = if in_conversation {
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
