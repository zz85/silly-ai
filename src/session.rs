//! Session manager - handles LLM, TTS, and audio playback

use crate::chat::Chat;
use crate::tts::Tts;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;

pub enum SessionCommand {
    UserInput(String),
    Greet,
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Thinking,
    Chunk(String),
    ResponseEnd,
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
    event_tx: mpsc::UnboundedSender<SessionEvent>,
}

impl SessionManager {
    pub fn new(
        chat: Chat,
        tts: Tts,
        tts_playing: Arc<AtomicBool>,
        event_tx: mpsc::UnboundedSender<SessionEvent>,
    ) -> Self {
        Self { chat, tts, tts_playing, event_tx }
    }

    pub async fn run(mut self, mut cmd_rx: mpsc::UnboundedReceiver<SessionCommand>) {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                SessionCommand::Greet => {
                    self.process_message("Hello.").await;
                }
                SessionCommand::UserInput(text) => {
                    self.process_message(&text).await;
                }
            }
        }
    }

    async fn process_message(&mut self, message: &str) {
        self.tts_playing.store(true, Ordering::SeqCst);
        let _ = self.event_tx.send(SessionEvent::Thinking);

        // Stream LLM response first
        self.chat.history_push_user(message);
        
        let mut sentences: Vec<String> = Vec::new();
        let mut full_response = String::new();

        match self.chat.create_stream().await {
            Ok(mut llm_stream) => {
                let mut buffer = String::new();

                while let Some(content) = llm_stream.next().await {
                    let _ = self.event_tx.send(SessionEvent::Chunk(content.clone()));
                    full_response.push_str(&content);
                    buffer.push_str(&content);

                    // Collect complete sentences for TTS
                    while let Some(pos) = buffer.find(|c| c == '.' || c == '!' || c == '?') {
                        let sentence = buffer[..=pos].trim().to_string();
                        if !sentence.is_empty() {
                            sentences.push(sentence);
                        }
                        buffer = buffer[pos + 1..].to_string();
                    }
                }

                // Flush remaining
                let remaining = buffer.trim();
                if !remaining.is_empty() {
                    sentences.push(remaining.to_string());
                }

                self.chat.history_push_assistant(&full_response);
            }
            Err(e) => {
                let _ = self.event_tx.send(SessionEvent::Error(e.to_string()));
                self.tts_playing.store(false, Ordering::SeqCst);
                return;
            }
        }

        let _ = self.event_tx.send(SessionEvent::ResponseEnd);
        let _ = self.event_tx.send(SessionEvent::ContextWords(self.chat.context_words()));

        // Now create audio and play TTS
        let (stream, sink) = match Tts::create_sink() {
            Ok(s) => s,
            Err(e) => {
                let _ = self.event_tx.send(SessionEvent::Error(e.to_string()));
                self.tts_playing.store(false, Ordering::SeqCst);
                return;
            }
        };

        for sentence in &sentences {
            let _ = self.tts.queue(sentence, &sink);
        }

        let _ = self.event_tx.send(SessionEvent::Speaking);

        // Wait for TTS playback
        while !sink.empty() {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        Tts::finish(stream, sink);
        let _ = self.event_tx.send(SessionEvent::SpeakingDone);
        let _ = self.event_tx.send(SessionEvent::Ready);
        self.tts_playing.store(false, Ordering::SeqCst);
    }
}
