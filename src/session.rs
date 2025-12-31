//! Session manager - handles LLM, TTS, and audio playback

use crate::chat::Chat;
use crate::tts::Tts;
use crate::stats::{SharedStats, Sample, StatKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
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

    pub async fn run(mut self, mut cmd_rx: mpsc::UnboundedReceiver<SessionCommand>) {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                SessionCommand::Greet => {
                    self.process_message("Hello.", &mut cmd_rx).await;
                }
                SessionCommand::UserInput(text) => {
                    self.process_message(&text, &mut cmd_rx).await;
                }
                SessionCommand::Cancel => {
                    // Nothing to cancel if idle
                }
            }
        }
    }

    async fn process_message(&mut self, message: &str, cmd_rx: &mut mpsc::UnboundedReceiver<SessionCommand>) {
        self.tts_playing.store(true, Ordering::SeqCst);
        let _ = self.event_tx.send(SessionEvent::Thinking);

        self.chat.history_push_user(message);
        
        let mut full_response = String::new();

        let stream_result = self.chat.create_stream().await;
        let mut llm_stream = match stream_result {
            Ok(s) => s,
            Err(e) => {
                let _ = self.event_tx.send(SessionEvent::Error(e.to_string()));
                self.tts_playing.store(false, Ordering::SeqCst);
                return;
            }
        };

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
        let mut cancelled = false;
        let mut speaking_sent = false;
        let llm_start = Instant::now();
        let input_len = message.len();

        loop {
            tokio::select! {
                chunk = llm_stream.next() => {
                    match chunk {
                        Some(content) => {
                            let _ = self.event_tx.send(SessionEvent::Chunk(content.clone()));
                            full_response.push_str(&content);
                            buffer.push_str(&content);

                            while let Some(pos) = buffer.find(|c| c == '.' || c == '!' || c == '?') {
                                let sentence = buffer[..=pos].trim().to_string();
                                if !sentence.is_empty() && self.tts_enabled.load(Ordering::SeqCst) {
                                    if !speaking_sent {
                                        let _ = self.event_tx.send(SessionEvent::Speaking);
                                        speaking_sent = true;
                                    }
                                    let _ = self.tts.queue(&sentence, &sink);
                                }
                                buffer = buffer[pos + 1..].to_string();
                            }
                        }
                        None => break,
                    }
                }
                cmd = cmd_rx.recv() => {
                    if let Some(SessionCommand::Cancel) = cmd {
                        cancelled = true;
                        sink.stop();
                        break;
                    }
                }
            }
        }

        // Record LLM stats
        if let Some(ref stats) = self.stats {
            let sample = Sample {
                duration: llm_start.elapsed(),
                input_size: input_len,
                output_size: full_response.split_whitespace().count(), // approx tokens
            };
            stats.lock().unwrap().llm.push(sample);
        }

        if cancelled {
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
                speaking_sent = true;
            }
            let _ = self.tts.queue(remaining, &sink);
        }

        self.chat.history_push_assistant(&full_response);

        let response_words = full_response.split_whitespace().count();
        let _ = self.event_tx.send(SessionEvent::ResponseEnd { response_words });
        let _ = self.event_tx.send(SessionEvent::ContextWords(self.chat.context_words()));

        // Wait for TTS with cancel check
        loop {
            if sink.empty() {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
                cmd = cmd_rx.recv() => {
                    if let Some(SessionCommand::Cancel) = cmd {
                        sink.stop();
                        break;
                    }
                }
            }
        }

        Tts::finish(stream, sink);
        let _ = self.event_tx.send(SessionEvent::SpeakingDone);
        let _ = self.event_tx.send(SessionEvent::Ready);
        self.tts_playing.store(false, Ordering::SeqCst);
    }
}
