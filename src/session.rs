//! Session manager - handles LLM, TTS, and audio playback

use crate::chat::Chat;
use crate::state::SharedState;
use crate::stats::{LlmTimer, SharedStats};
use crate::tts::Tts;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    tts_playing: Arc<AtomicBool>,
    tts_enabled: Arc<AtomicBool>,
    event_tx: mpsc::UnboundedSender<SessionEvent>,
    stats: Option<SharedStats>,
    state: Option<SharedState>,
}

impl SessionManager {
    pub fn new(
        chat: Chat,
        tts: Tts,
        tts_playing: Arc<AtomicBool>,
        tts_enabled: Arc<AtomicBool>,
        event_tx: mpsc::UnboundedSender<SessionEvent>,
    ) -> Self {
        Self {
            chat,
            tts,
            tts_playing,
            tts_enabled,
            event_tx,
            stats: None,
            state: None,
        }
    }

    pub fn with_stats(mut self, stats: SharedStats) -> Self {
        self.stats = Some(stats);
        self
    }

    pub fn with_state(mut self, state: SharedState) -> Self {
        self.state = Some(state);
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
        if let Some(ref state) = self.state {
            state.clear_cancel();
        }

        self.tts_playing.store(true, Ordering::SeqCst);
        if let Some(ref state) = self.state {
            state.tts_playing.store(true, Ordering::SeqCst);
        }
        let _ = self.event_tx.send(SessionEvent::Thinking);

        self.chat.history_push_user(message);

        // Create TTS controller if state is available, otherwise use legacy sink
        let _use_controller = self.state.is_some();
        let (stream, controller, sink) = if let Some(ref state) = self.state {
            match Tts::create_controller(Arc::clone(state)) {
                Ok((s, c)) => (s, Some(c), None),
                Err(e) => {
                    let _ = self.event_tx.send(SessionEvent::Error(e.to_string()));
                    self.tts_playing.store(false, Ordering::SeqCst);
                    state.tts_playing.store(false, Ordering::SeqCst);
                    return;
                }
            }
        } else {
            match Tts::create_sink() {
                Ok((s, sink)) => (s, None, Some(sink)),
                Err(e) => {
                    let _ = self.event_tx.send(SessionEvent::Error(e.to_string()));
                    self.tts_playing.store(false, Ordering::SeqCst);
                    return;
                }
            }
        };

        let mut buffer = String::new();
        let mut speaking_sent = false;
        let mut llm_timer = self.stats.as_ref().map(|s| LlmTimer::new(Arc::clone(s)));
        let mut full_response = String::new();

        let event_tx = self.event_tx.clone();
        let tts_enabled = Arc::clone(&self.tts_enabled);

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

                if !sentence_content.is_empty() && tts_enabled.load(Ordering::SeqCst) {
                    if !speaking_sent {
                        let _ = event_tx.send(SessionEvent::Speaking);
                        speaking_sent = true;
                    }
                    // Use controller or legacy sink
                    if let Some(ref ctrl) = controller {
                        let _ = self.tts.queue_to_controller(sentence_content, ctrl);
                    } else if let Some(ref s) = sink {
                        let _ = self.tts.queue(sentence_content, s);
                    }
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
            if let Some(ctrl) = controller {
                ctrl.stop();
                Tts::finish_controller(stream, ctrl);
            } else if let Some(s) = sink {
                Tts::finish(stream, s);
            }
            self.tts_playing.store(false, Ordering::SeqCst);
            if let Some(ref state) = self.state {
                state.tts_playing.store(false, Ordering::SeqCst);
            }
            let _ = self.event_tx.send(SessionEvent::Ready);
            return;
        }

        // Flush remaining
        let remaining = buffer.trim();
        if !remaining.is_empty() && self.tts_enabled.load(Ordering::SeqCst) {
            if !speaking_sent {
                let _ = self.event_tx.send(SessionEvent::Speaking);
            }
            if let Some(ref ctrl) = controller {
                let _ = self.tts.queue_to_controller(remaining, ctrl);
            } else if let Some(ref s) = sink {
                let _ = self.tts.queue(remaining, s);
            }
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
        if let Some(ctrl) = controller {
            // Poll for completion with cancel check
            while ctrl.is_playing() {
                // Check for cancel request
                if ctrl.is_cancel_requested() {
                    ctrl.stop();
                    if let Some(ref state) = self.state {
                        state.clear_cancel();
                    }
                    break;
                }
                // Update volume based on state (for ducking)
                ctrl.update_volume();
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Tts::finish_controller(stream, ctrl);
        } else if let Some(s) = sink {
            s.sleep_until_end();
            Tts::finish(stream, s);
        }

        let _ = self.event_tx.send(SessionEvent::SpeakingDone);
        let _ = self.event_tx.send(SessionEvent::Ready);
        self.tts_playing.store(false, Ordering::SeqCst);
        if let Some(ref state) = self.state {
            state.tts_playing.store(false, Ordering::SeqCst);
        }
    }
}
