//! Session manager - handles LLM, TTS, and audio playback

use crate::chat::Chat;
use crate::state::SharedState;
use crate::stats::{LlmTimer, SharedStats};
use crate::tts::Tts;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;

pub enum SessionCommand {
    UserInput(String),
    Greet,
    Cancel,
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Thinking,
    Chunk(String),
    ResponseEnd { response_words: usize },
    Speaking,
    SpeakingDone,
    ContextWords(usize),
    Ready,
    Error(String),
}

pub struct SessionManager {
    chat: Chat,
    tts: Tts,
    event_tx: mpsc::UnboundedSender<SessionEvent>,
    stats: Option<SharedStats>,
    state: SharedState,
}

impl SessionManager {
    pub fn new(
        chat: Chat,
        tts: Tts,
        state: SharedState,
        event_tx: mpsc::UnboundedSender<SessionEvent>,
    ) -> Self {
        Self {
            chat,
            tts,
            event_tx,
            stats: None,
            state,
        }
    }

    pub fn with_stats(mut self, stats: SharedStats) -> Self {
        self.stats = Some(stats);
        self
    }

    pub fn run_sync(mut self, mut cmd_rx: mpsc::UnboundedReceiver<SessionCommand>) {
        while let Some(cmd) = cmd_rx.blocking_recv() {
            match cmd {
                SessionCommand::Greet => {
                    self.process_message("Hello.");
                }
                SessionCommand::UserInput(text) => {
                    self.process_message(&text);
                }
                SessionCommand::Cancel => {
                    // Nothing to cancel if idle
                }
            }
        }
    }

    fn process_message(&mut self, message: &str) {
        // Clear any previous cancel request
        self.state.clear_cancel();

        self.state.tts_playing.store(true, Ordering::SeqCst);
        let _ = self.event_tx.send(SessionEvent::Thinking);

        self.chat.history_push_user(message);

        // Create TTS controller with state
        let (stream, controller) = match Tts::create_controller(Arc::clone(&self.state)) {
            Ok((s, c)) => (s, c),
            Err(e) => {
                let _ = self.event_tx.send(SessionEvent::Error(e.to_string()));
                self.state.tts_playing.store(false, Ordering::SeqCst);
                return;
            }
        };

        let mut buffer = String::new();
        let mut speaking_sent = false;
        let mut llm_timer = self.stats.as_ref().map(|s| LlmTimer::new(Arc::clone(s)));
        let mut full_response = String::new();

        let event_tx = self.event_tx.clone();
        let state = Arc::clone(&self.state);

        // Generate with streaming callback
        let result = self.chat.generate(|token| {
            if let Some(ref mut timer) = llm_timer {
                timer.mark_first_token();
            }
            let _ = event_tx.send(SessionEvent::Chunk(token.to_string()));
            full_response.push_str(token);
            buffer.push_str(token);

            // Queue complete sentences to TTS - improved sentence detection
            let mut start_pos = 0;
            while let Some(pos) = buffer[start_pos..].find(|c| c == '.' || c == '!' || c == '?') {
                let actual_pos = start_pos + pos;
                // Check if this is a sentence ending or just punctuation in the middle of text
                let sentence_end = actual_pos + 1;
                let sentence_content = &buffer[start_pos..sentence_end].trim();

                // Skip if it's just a single character (e.g., "U.S.A." or "Dr.")
                if sentence_content
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || c == '.')
                {
                    start_pos = sentence_end;
                    continue;
                }

                if !sentence_content.is_empty() && state.tts_enabled.load(Ordering::SeqCst) {
                    if !speaking_sent {
                        let _ = event_tx.send(SessionEvent::Speaking);
                        speaking_sent = true;
                    }
                    let _ = self.tts.queue_to_controller(sentence_content, &controller);
                }
                buffer = buffer[sentence_end..].to_string();
                start_pos = 0;
            }
        });

        // Record LLM stats
        let token_count = full_response.split_whitespace().count();
        if let Some(timer) = llm_timer {
            timer.finish(token_count);
        }

        if let Err(e) = result {
            let _ = self.event_tx.send(SessionEvent::Error(e.to_string()));
            self.chat.history_pop();
            controller.stop();
            Tts::finish_controller(stream, controller);
            self.state.tts_playing.store(false, Ordering::SeqCst);
            let _ = self.event_tx.send(SessionEvent::Ready);
            return;
        }

        // Flush remaining
        let remaining = buffer.trim();
        if !remaining.is_empty() && self.state.tts_enabled.load(Ordering::SeqCst) {
            if !speaking_sent {
                let _ = self.event_tx.send(SessionEvent::Speaking);
            }
            let _ = self.tts.queue_to_controller(remaining, &controller);
        }

        self.chat.history_push_assistant(&full_response);

        let response_words = full_response.split_whitespace().count();
        let _ = self
            .event_tx
            .send(SessionEvent::ResponseEnd { response_words });
        let _ = self
            .event_tx
            .send(SessionEvent::ContextWords(self.chat.context_words()));

        // Wait for TTS to finish with cancel support
        // Poll for completion with cancel check
        while controller.is_playing() {
            // Check for cancel request
            if controller.is_cancel_requested() {
                controller.stop();
                self.state.clear_cancel();
                break;
            }
            // Update volume based on state (for ducking)
            controller.update_volume();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Tts::finish_controller(stream, controller);

        let _ = self.event_tx.send(SessionEvent::SpeakingDone);
        let _ = self.event_tx.send(SessionEvent::Ready);
        self.state.tts_playing.store(false, Ordering::SeqCst);
    }
}
