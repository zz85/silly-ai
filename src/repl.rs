//! REPL input handling - keyboard and voice input processing

use crate::chat::Chat;
use crate::render::Ui;
use crate::tts::Tts;
use crate::wake::WakeWord;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use rustyline::hint::Hint;
use rustyline::Editor;
use rustyline::highlight::Highlighter;
use rustyline::validate::Validator;
use rustyline::completion::Completer;
use rustyline::hint::Hinter;
use rustyline::Helper;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Events from audio transcription pipeline
pub enum TranscriptEvent {
    Preview(String),
    Final(String),
}

/// Shared preview text for rustyline hinter
#[derive(Clone)]
pub struct PreviewHint {
    text: Arc<Mutex<String>>,
}

impl PreviewHint {
    pub fn new() -> Self {
        Self { text: Arc::new(Mutex::new(String::new())) }
    }

    pub fn set(&self, s: &str) {
        if let Ok(mut t) = self.text.lock() {
            *t = s.to_string();
        }
    }

    pub fn clear(&self) {
        if let Ok(mut t) = self.text.lock() {
            t.clear();
        }
    }
}

#[derive(Clone)]
struct PreviewHintStr(String);

impl Hint for PreviewHintStr {
    fn display(&self) -> &str { &self.0 }
    fn completion(&self) -> Option<&str> { None }
}

struct ReplHelper {
    hint: PreviewHint,
}

impl Helper for ReplHelper {}
impl Completer for ReplHelper {
    type Candidate = String;
}
impl Highlighter for ReplHelper {}
impl Validator for ReplHelper {}

impl Hinter for ReplHelper {
    type Hint = PreviewHintStr;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &rustyline::Context<'_>) -> Option<Self::Hint> {
        if let Ok(t) = self.hint.text.lock() {
            if !t.is_empty() {
                return Some(PreviewHintStr(format!("  [{}]", t)));
            }
        }
        None
    }
}

/// Spawns readline thread, returns channels for input/prefill and preview handle
pub fn spawn_readline() -> (flume::Receiver<String>, flume::Sender<String>, PreviewHint) {
    let (input_tx, input_rx) = flume::unbounded::<String>();
    let (prefill_tx, prefill_rx) = flume::unbounded::<String>();
    let hint = PreviewHint::new();
    let hint_clone = hint.clone();

    std::thread::spawn(move || {
        let helper = ReplHelper { hint: hint_clone };
        let mut rl = Editor::new().expect("Failed to create readline");
        rl.set_helper(Some(helper));

        loop {
            let initial = prefill_rx.try_recv().unwrap_or_default();

            let result = if initial.is_empty() {
                rl.readline("> ")
            } else {
                rl.readline_with_initial("> ", (&initial, ""))
            };

            match result {
                Ok(line) => {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        let _ = rl.add_history_entry(&line);
                    }
                    if input_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    (input_rx, prefill_tx, hint)
}

/// Process a command: send to LLM, stream response, play TTS
pub async fn process_command(
    command: &str,
    tts_playing: &Arc<AtomicBool>,
    tts_engine: &Tts,
    chat: &mut Chat,
    ui: &Ui,
) -> Option<Instant> {
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
            wait_for_playback(&sink);
            Tts::finish(stream, sink);
            ui.speaking_done();
        }
        Err(e) => {
            eprintln!("Audio error: {}", e);
            let ui_resp = ui.clone();
            let _ = chat
                .send_streaming_with_callback(command, |_| {}, |chunk| ui_resp.append_response(chunk))
                .await;
            ui.end_response();
            ui.set_context_words(chat.context_words());
        }
    }

    tts_playing.store(false, Ordering::SeqCst);
    Some(Instant::now())
}

/// Wait for TTS playback, handle pause/stop keys
fn wait_for_playback(sink: &rodio::Sink) {
    let mut paused = false;
    while !sink.empty() {
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char(' ') => {
                            paused = !paused;
                            if paused { sink.pause(); } else { sink.play(); }
                        }
                        KeyCode::Esc | KeyCode::Char('q') => {
                            sink.stop();
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// Handle voice transcript event
pub fn handle_transcript(
    event: TranscriptEvent,
    wake_word: &WakeWord,
    last_interaction: Option<Instant>,
    wake_timeout: Duration,
    pending: &mut Option<String>,
    deadline: &mut Option<tokio::time::Instant>,
    preview_hint: &PreviewHint,
) {
    const EDIT_DELAY: Duration = Duration::from_millis(800);

    match event {
        TranscriptEvent::Preview(text) => {
            preview_hint.set(&text);
        }
        TranscriptEvent::Final(text) => {
            let in_conversation = last_interaction
                .map(|t| t.elapsed() < wake_timeout)
                .unwrap_or(false);

            let command = if in_conversation {
                text
            } else {
                match wake_word.detect(&text) {
                    Some(cmd) => cmd,
                    None => {
                        preview_hint.clear();
                        return;
                    }
                }
            };

            if command.is_empty() {
                preview_hint.clear();
                return;
            }

            // Append to pending buffer
            if let Some(p) = pending {
                p.push(' ');
                p.push_str(&command);
            } else {
                *pending = Some(command);
            }
            *deadline = Some(tokio::time::Instant::now() + EDIT_DELAY);

            preview_hint.set(&format!("▶ {}", pending.as_ref().unwrap()));
        }
    }
}
