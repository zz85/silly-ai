//! Voice-to-keyboard typing with command recognition
//!
//! Enables dictation into any application with voice commands for
//! punctuation, navigation, and editing.
//!
//! # Features
//!
//! - **Smart command detection**: Distinguishes between text and commands based on
//!   pause duration, phrase length, and pattern matching
//! - **Inline punctuation**: "hello comma world" becomes "hello, world"
//! - **Navigation commands**: "go to end of line", "select all", etc.
//! - **Undo/Redo support**: Tracks typed operations for reversal
//! - **Configurable input method**: Clipboard+paste (default) or direct typing
//! - **Global hotkeys**: Double-tap Cmd or Ctrl+Space to toggle

mod commands;
mod hotkey;
mod input;
mod processor;

pub use commands::CommandParser;
pub use hotkey::{start_hotkey_listener, HotkeyConfig, HotkeyEvent};
pub use input::InputMethod;
pub use processor::{ProcessResult, TypingProcessor};
