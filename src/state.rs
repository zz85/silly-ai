//! Shared runtime state - centralized, thread-safe state accessible from all components
//!
//! This replaces the scattered `Arc<AtomicBool>` flags and provides a single source of truth
//! for all runtime-configurable settings.

#![allow(dead_code)] // Many fields/methods will be used in future phases

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

use crate::config::Config;

/// Application modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AppMode {
    /// Conversational mode (default, no wake word needed)
    Chat = 0,
    /// Paused conversation (requires wake word to resume)
    Paused = 1,
    /// Transcription-only mode (no LLM)
    Transcribe = 2,
    /// Note-taking mode
    NoteTaking = 3,
    /// Command-only mode (no LLM, only processes commands)
    Command = 4,
}

impl From<u8> for AppMode {
    fn from(v: u8) -> Self {
        match v {
            0 => AppMode::Chat,
            1 => AppMode::Paused,
            2 => AppMode::Transcribe,
            3 => AppMode::NoteTaking,
            4 => AppMode::Command,
            _ => AppMode::Chat,
        }
    }
}

impl fmt::Display for AppMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppMode::Chat => write!(f, "Chat"),
            AppMode::Paused => write!(f, "Paused"),
            AppMode::Transcribe => write!(f, "Transcribe"),
            AppMode::NoteTaking => write!(f, "Note"),
            AppMode::Command => write!(f, "Command"),
        }
    }
}

/// Thread-safe f32 using bit casting to AtomicU32
#[derive(Debug)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }

    pub fn load(&self, order: Ordering) -> f32 {
        f32::from_bits(self.0.load(order))
    }

    pub fn store(&self, v: f32, order: Ordering) {
        self.0.store(v.to_bits(), order);
    }
}

/// Shared runtime state - accessible from all components
///
/// All fields use atomic operations for thread-safe access without locks.
/// Pass `Arc<RuntimeState>` to components that need to read or modify state.
pub struct RuntimeState {
    // ========================================================================
    // Audio state
    // ========================================================================
    /// Microphone is muted (no audio processing)
    pub mic_muted: AtomicBool,
    /// Current microphone RMS level (0.0-1.0)
    pub mic_level: AtomicF32,

    // ========================================================================
    // TTS state
    // ========================================================================
    /// TTS output is enabled
    pub tts_enabled: AtomicBool,
    /// TTS is currently playing audio
    pub tts_playing: AtomicBool,
    /// Current TTS volume (0.0-1.0)
    pub tts_volume: AtomicF32,
    /// Current TTS output RMS level (0.0-1.0)
    pub tts_level: AtomicF32,
    /// Duck volume level from config
    duck_volume: AtomicF32,

    // ========================================================================
    // Interaction state
    // ========================================================================
    /// Continue processing audio while TTS is playing
    pub crosstalk_enabled: AtomicBool,
    /// Acoustic echo cancellation enabled
    pub aec_enabled: AtomicBool,
    /// Require wake word to activate
    pub wake_enabled: AtomicBool,
    /// Currently in an active conversation (within wake timeout)
    pub in_conversation: AtomicBool,
    /// Timestamp of last interaction (Unix ms)
    pub last_interaction_ms: AtomicU64,
    /// Wake timeout in seconds
    wake_timeout_secs: AtomicU64,

    // ========================================================================
    // Mode state
    // ========================================================================
    /// Current application mode (stored as u8)
    mode: AtomicU8,

    // ========================================================================
    // LLM state
    // ========================================================================
    /// LLM is currently generating a response
    pub llm_generating: AtomicBool,

    // ========================================================================
    // Cancellation
    // ========================================================================
    /// Cancel current operation requested
    pub cancel_requested: AtomicBool,
}

impl RuntimeState {
    /// Create new RuntimeState initialized from config
    pub fn new(config: &Config) -> Arc<Self> {
        Arc::new(Self {
            // Audio
            mic_muted: AtomicBool::new(false),
            mic_level: AtomicF32::new(0.0),

            // TTS
            tts_enabled: AtomicBool::new(true),
            tts_playing: AtomicBool::new(false),
            tts_volume: AtomicF32::new(1.0),
            tts_level: AtomicF32::new(0.0),
            duck_volume: AtomicF32::new(config.interaction.duck_volume),

            // Interaction
            crosstalk_enabled: AtomicBool::new(config.interaction.crosstalk),
            aec_enabled: AtomicBool::new(config.interaction.aec),
            wake_enabled: AtomicBool::new(true),
            in_conversation: AtomicBool::new(false),
            last_interaction_ms: AtomicU64::new(0),
            wake_timeout_secs: AtomicU64::new(config.wake_timeout_secs),

            // Mode - start in Chat mode by default
            mode: AtomicU8::new(AppMode::Chat as u8),

            // LLM
            llm_generating: AtomicBool::new(false),

            // Cancellation
            cancel_requested: AtomicBool::new(false),
        })
    }

    // ========================================================================
    // Mode helpers
    // ========================================================================

    /// Get current application mode
    pub fn mode(&self) -> AppMode {
        AppMode::from(self.mode.load(Ordering::SeqCst))
    }

    /// Set application mode
    pub fn set_mode(&self, mode: AppMode) {
        self.mode.store(mode as u8, Ordering::SeqCst);
    }

    // ========================================================================
    // Audio processing helpers
    // ========================================================================

    /// Check if audio should be processed (not muted, and either crosstalk or TTS not playing)
    pub fn should_process_audio(&self) -> bool {
        !self.mic_muted.load(Ordering::SeqCst)
            && (self.crosstalk_enabled.load(Ordering::SeqCst)
                || !self.tts_playing.load(Ordering::SeqCst))
    }

    /// Update microphone level
    pub fn set_mic_level(&self, level: f32) {
        self.mic_level.store(level, Ordering::SeqCst);
    }

    /// Get current microphone level
    pub fn get_mic_level(&self) -> f32 {
        self.mic_level.load(Ordering::SeqCst)
    }

    // ========================================================================
    // TTS volume helpers
    // ========================================================================

    /// Duck TTS volume (reduce to configured duck_volume)
    pub fn duck_tts(&self) {
        let duck = self.duck_volume.load(Ordering::SeqCst);
        self.tts_volume.store(duck, Ordering::SeqCst);
    }

    /// Restore TTS volume to full
    pub fn restore_tts_volume(&self) {
        self.tts_volume.store(1.0, Ordering::SeqCst);
    }

    /// Get current TTS volume
    pub fn get_tts_volume(&self) -> f32 {
        self.tts_volume.load(Ordering::SeqCst)
    }

    /// Set TTS volume
    pub fn set_tts_volume(&self, volume: f32) {
        self.tts_volume
            .store(volume.clamp(0.0, 1.0), Ordering::SeqCst);
    }

    /// Update TTS output level
    pub fn set_tts_level(&self, level: f32) {
        self.tts_level.store(level, Ordering::SeqCst);
    }

    /// Get current TTS output level
    pub fn get_tts_level(&self) -> f32 {
        self.tts_level.load(Ordering::SeqCst)
    }

    // ========================================================================
    // Cancellation helpers
    // ========================================================================

    /// Request cancellation of current operation
    pub fn request_cancel(&self) {
        self.cancel_requested.store(true, Ordering::SeqCst);
    }

    /// Clear cancellation request
    pub fn clear_cancel(&self) {
        self.cancel_requested.store(false, Ordering::SeqCst);
    }

    /// Check if cancellation was requested
    pub fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::SeqCst)
    }

    // ========================================================================
    // Interaction timing helpers
    // ========================================================================

    /// Update last interaction timestamp to now
    pub fn update_last_interaction(&self) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        self.last_interaction_ms.store(now, Ordering::SeqCst);
        self.in_conversation.store(true, Ordering::SeqCst);
    }

    /// Check if we're within the wake timeout window
    pub fn is_in_wake_timeout(&self) -> bool {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let last = self.last_interaction_ms.load(Ordering::SeqCst);
        let timeout_secs = self.wake_timeout_secs.load(Ordering::SeqCst);
        now - last < timeout_secs * 1000
    }

    /// Update conversation state based on timeout
    pub fn update_conversation_state(&self) {
        let in_timeout = self.is_in_wake_timeout();
        self.in_conversation.store(in_timeout, Ordering::SeqCst);
    }

    // ========================================================================
    // Toggle helpers (for commands)
    // ========================================================================

    /// Toggle microphone mute state, returns new state
    pub fn toggle_mic_mute(&self) -> bool {
        let current = self.mic_muted.load(Ordering::SeqCst);
        let new_state = !current;
        self.mic_muted.store(new_state, Ordering::SeqCst);
        new_state
    }

    /// Toggle TTS enabled state, returns new state
    pub fn toggle_tts(&self) -> bool {
        let current = self.tts_enabled.load(Ordering::SeqCst);
        let new_state = !current;
        self.tts_enabled.store(new_state, Ordering::SeqCst);
        new_state
    }

    /// Toggle crosstalk enabled state, returns new state
    pub fn toggle_crosstalk(&self) -> bool {
        let current = self.crosstalk_enabled.load(Ordering::SeqCst);
        let new_state = !current;
        self.crosstalk_enabled.store(new_state, Ordering::SeqCst);
        new_state
    }

    /// Toggle AEC enabled state, returns new state
    pub fn toggle_aec(&self) -> bool {
        let current = self.aec_enabled.load(Ordering::SeqCst);
        let new_state = !current;
        self.aec_enabled.store(new_state, Ordering::SeqCst);
        new_state
    }

    /// Toggle wake word requirement, returns new state
    pub fn toggle_wake(&self) -> bool {
        let current = self.wake_enabled.load(Ordering::SeqCst);
        let new_state = !current;
        self.wake_enabled.store(new_state, Ordering::SeqCst);
        new_state
    }
}

impl fmt::Debug for RuntimeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeState")
            .field("mode", &self.mode())
            .field("mic_muted", &self.mic_muted.load(Ordering::SeqCst))
            .field("tts_enabled", &self.tts_enabled.load(Ordering::SeqCst))
            .field("tts_playing", &self.tts_playing.load(Ordering::SeqCst))
            .field(
                "crosstalk_enabled",
                &self.crosstalk_enabled.load(Ordering::SeqCst),
            )
            .field("aec_enabled", &self.aec_enabled.load(Ordering::SeqCst))
            .field("wake_enabled", &self.wake_enabled.load(Ordering::SeqCst))
            .field(
                "in_conversation",
                &self.in_conversation.load(Ordering::SeqCst),
            )
            .field(
                "llm_generating",
                &self.llm_generating.load(Ordering::SeqCst),
            )
            .field(
                "cancel_requested",
                &self.cancel_requested.load(Ordering::SeqCst),
            )
            .finish()
    }
}

/// Type alias for shared state
pub type SharedState = Arc<RuntimeState>;
