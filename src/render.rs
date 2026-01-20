//! UI event types and sender for cross-thread communication

use crate::state::AppMode;
use std::io;
use std::fs::OpenOptions;
use std::io::Write;

fn debug_log(msg: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.log")
    {
        let _ = writeln!(file, "{}: {}", chrono::Utc::now().format("%H:%M:%S%.3f"), msg);
    }
}

#[derive(Clone, Debug)]
pub enum UiEvent {
    Preview(String),
    Final(String),
    Thinking,
    Speaking,
    SpeakingDone,
    ResponseChunk(String),
    ResponseEnd,
    Idle,
    Tick,
    ContextWords(usize),
    SwitchUiMode(UiMode),
}

/// UI mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum UiMode {
    /// Text-based terminal UI (default)
    #[default]
    Text,
    /// Graphical orb visualization
    Graphical,
}

/// Visual style for the graphical orb UI
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrbStyle {
    /// Volumetric noise blob
    #[default]
    Blob,
    /// Simple rotating ring
    Ring,
    /// Concentric glowing orbs (horizontal ellipses)
    Orbs,
    /// Particle sphere with displacement and noise
    Sphere,
}

/// Trait for UI renderers - allows swapping between text and graphical UI
pub trait UiRenderer: Send {
    /// Handle a UI event from the event channel
    fn handle_ui_event(&mut self, event: UiEvent) -> io::Result<()>;

    /// Draw/render the current UI state
    fn draw(&mut self) -> io::Result<()>;

    /// Poll for keyboard input, returns submitted text if any
    fn poll_input(&mut self) -> io::Result<Option<String>>;

    /// Restore terminal state on exit
    fn restore(&self) -> io::Result<()>;

    /// Show a multi-line message
    fn show_message(&mut self, text: &str);

    /// Set auto-submit progress (0.0-1.0, None to disable)
    fn set_auto_submit_progress(&mut self, progress: Option<f32>);

    /// Set microphone muted state indicator
    fn set_mic_muted(&mut self, muted: bool);

    /// Set TTS enabled state indicator
    fn set_tts_enabled(&mut self, enabled: bool);

    /// Set wake word enabled state indicator
    fn set_wake_enabled(&mut self, enabled: bool);

    /// Set current application mode
    fn set_mode(&mut self, mode: AppMode);

    /// Set the ready state
    fn set_ready(&mut self);

    /// Set last response word count
    fn set_last_response_words(&mut self, words: usize);

    /// Set current audio input level (0.0-1.0)
    fn set_audio_level(&mut self, level: f32);

    /// Set current TTS output level (0.0-1.0)
    fn set_tts_level(&mut self, level: f32);

    /// Check if there was input activity since last call
    fn has_input_activity(&mut self) -> bool;

    /// Check if there was any keypress since last call
    fn has_keypress_activity(&mut self) -> bool;

    /// Take the current input buffer and clear it
    fn take_input(&mut self) -> Option<String>;

    /// Append text to the input buffer
    fn append_input(&mut self, text: &str);

    /// Get the current UI mode
    #[allow(dead_code)]
    fn ui_mode(&self) -> UiMode;

    /// Switch to a different visual style (for graphical UI)
    fn set_visual_style(&mut self, _style: OrbStyle) {
        // Default no-op for text UI
    }

    /// Downcast to Any for type checking
    fn as_any(&self) -> &dyn std::any::Any;

    /// Downcast to Any for mutable type checking
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

#[derive(Clone)]
pub struct Ui {
    tx: flume::Sender<UiEvent>,
}

impl Ui {
    pub fn new() -> (Self, flume::Receiver<UiEvent>) {
        let (tx, rx) = flume::unbounded();
        (Self { tx }, rx)
    }

    pub fn set_preview(&self, text: String) {
        let _ = self.tx.send(UiEvent::Preview(text));
    }

    pub fn set_thinking(&self) {
        let _ = self.tx.send(UiEvent::Thinking);
    }

    pub fn set_speaking(&self) {
        let _ = self.tx.send(UiEvent::Speaking);
    }

    pub fn set_idle(&self) {
        let _ = self.tx.send(UiEvent::Idle);
    }

    pub fn show_final(&self, text: &str) {
        let _ = self.tx.send(UiEvent::Final(text.to_string()));
    }

    pub fn append_response(&self, text: &str) {
        let _ = self.tx.send(UiEvent::ResponseChunk(text.to_string()));
    }

    pub fn end_response(&self) {
        let _ = self.tx.send(UiEvent::ResponseEnd);
    }

    pub fn speaking_done(&self) {
        let _ = self.tx.send(UiEvent::SpeakingDone);
    }

    pub fn tick(&self) {
        let _ = self.tx.send(UiEvent::Tick);
    }

    pub fn set_context_words(&self, count: usize) {
        let _ = self.tx.send(UiEvent::ContextWords(count));
    }

    pub fn request_ui_mode_switch(&self, mode: UiMode) {
        debug_log(&format!("request_ui_mode_switch called with mode: {:?}", mode));
        let _ = self.tx.send(UiEvent::SwitchUiMode(mode));
        debug_log("SwitchUiMode event sent");
    }
}
