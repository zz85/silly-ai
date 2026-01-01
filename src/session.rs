//! Session manager - handles LLM, TTS, and audio playback

use crate::chat::Chat;
use crate::tts::Tts;
use crate::stats::{SharedStats, LlmTimer};
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
}

impl SessionManager {
    pub fn new(
        chat: Chat,
        tts: Tts,
        tts_playing: Arc<AtomicBool>,
        tts_enabled: Arc<AtomicBool>,
        event_tx: mpsc::UnboundedSender<SessionEvent>,
    ) -> Self {
        Self { chat, tts, tts_playing, tts_enabled, event_tx, stats: None }
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
        self.tts_playing.store(true, Ordering::SeqCst);
        let _ = self.event_tx.send(SessionEvent::Thinking);

        self.chat.history_push_user(message);

        // Create TTS sink upfront for streaming TTS
        let (stream, sink) = match Tts::create_sink() {
            Ok(s) => s,
            Err(e) => {
                let _ = self.event_tx.send(SessionEvent::Error(e.to_string()));
                self.tts_playing.store(false, Ordering::SeqCst);
                return;
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

            // Queue complete sentences to TTS
            while let Some(pos) = buffer.find(|c| c == '.' || c == '!' || c == '?') {
                let sentence = buffer[..=pos].trim().to_string();
                if !sentence.is_empty() && tts_enabled.load(Ordering::SeqCst) {
                    if !speaking_sent {
                        let _ = event_tx.send(SessionEvent::Speaking);
                        speaking_sent = true;
                    }
                    let _ = self.tts.queue(&sentence, &sink);
                }
                buffer = buffer[pos + 1..].to_string();
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
            Tts::finish(stream, sink);
            self.tts_playing.store(false, Ordering::SeqCst);
            let _ = self.event_tx.send(SessionEvent::Ready);
            return;
        }

        // Flush remaining
        let remaining = buffer.trim();
        if !remaining.is_empty() && self.tts_enabled.load(Ordering::SeqCst) {
            if !speaking_sent {
                let _ = self.event_tx.send(SessionEvent::Speaking);
            }
            let _ = self.tts.queue(remaining, &sink);
        }

        self.chat.history_push_assistant(&full_response);

        let response_words = full_response.split_whitespace().count();
        let _ = self.event_tx.send(SessionEvent::ResponseEnd { response_words });
        let _ = self.event_tx.send(SessionEvent::ContextWords(self.chat.context_words()));

        // Wait for TTS to finish
        sink.sleep_until_end();

        Tts::finish(stream, sink);
        let _ = self.event_tx.send(SessionEvent::SpeakingDone);
        let _ = self.event_tx.send(SessionEvent::Ready);
        self.tts_playing.store(false, Ordering::SeqCst);
    }
}
