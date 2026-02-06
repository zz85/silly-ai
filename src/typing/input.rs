//! Keyboard input methods using enigo
//!
//! Provides two methods for typing text into applications:
//! - **Clipboard**: Copy text to clipboard, then send Cmd/Ctrl+V (more reliable)
//! - **Direct**: Use enigo's native text input (faster, but may fail with some characters)

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::thread;
use std::time::Duration;

/// Input method for typing text
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum InputMethod {
    /// Use enigo's native text input directly (default, more reliable on macOS)
    #[default]
    Direct,
    /// Copy to clipboard, then paste with Cmd/Ctrl+V (may have issues)
    Clipboard,
}

impl InputMethod {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "clipboard" => InputMethod::Clipboard,
            _ => InputMethod::Direct,
        }
    }
}

/// Error type for typing operations
#[derive(Debug)]
pub enum TypingError {
    Enigo(String),
    Clipboard(String),
}

impl std::fmt::Display for TypingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypingError::Enigo(msg) => write!(f, "Enigo error: {}", msg),
            TypingError::Clipboard(msg) => write!(f, "Clipboard error: {}", msg),
        }
    }
}

impl std::error::Error for TypingError {}

/// Keyboard input handler using enigo
pub struct TypingInput {
    enigo: Enigo,
    clipboard: Clipboard,
    method: InputMethod,
}

impl TypingInput {
    /// Create a new typing input handler
    pub fn new(method: InputMethod) -> Result<Self, TypingError> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| TypingError::Enigo(format!("Failed to initialize Enigo: {}", e)))?;
        let clipboard = Clipboard::new().map_err(|e| {
            TypingError::Clipboard(format!("Failed to initialize clipboard: {}", e))
        })?;

        Ok(Self {
            enigo,
            clipboard,
            method,
        })
    }

    /// Type text using the configured method
    pub fn type_text(&mut self, text: &str) -> Result<(), TypingError> {
        if text.is_empty() {
            return Ok(());
        }

        match self.method {
            InputMethod::Direct => self.type_direct(text),
            InputMethod::Clipboard => {
                // Try clipboard, fall back to direct if it fails
                match self.type_via_clipboard(text) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        eprintln!("[TYPING] Clipboard method failed: {}, trying direct", e);
                        self.type_direct(text)
                    }
                }
            }
        }
    }

    /// Send a single key press
    pub fn send_key(&mut self, key: Key) -> Result<(), TypingError> {
        self.enigo
            .key(key, Direction::Click)
            .map_err(|e| TypingError::Enigo(format!("Failed to send key: {}", e)))
    }

    /// Send key with modifiers (e.g., Cmd+Z for undo)
    pub fn send_key_combo(&mut self, modifiers: &[Key], key: Key) -> Result<(), TypingError> {
        // Press all modifiers
        for modifier in modifiers {
            self.enigo
                .key(*modifier, Direction::Press)
                .map_err(|e| TypingError::Enigo(format!("Failed to press modifier: {}", e)))?;
        }

        // Small delay for modifier to register
        thread::sleep(Duration::from_millis(10));

        // Click the main key
        self.enigo
            .key(key, Direction::Click)
            .map_err(|e| TypingError::Enigo(format!("Failed to click key: {}", e)))?;

        // Small delay before releasing
        thread::sleep(Duration::from_millis(50));

        // Release all modifiers in reverse order
        for modifier in modifiers.iter().rev() {
            self.enigo
                .key(*modifier, Direction::Release)
                .map_err(|e| TypingError::Enigo(format!("Failed to release modifier: {}", e)))?;
        }

        Ok(())
    }

    /// Get the platform-specific modifier key (Cmd on macOS, Ctrl elsewhere)
    pub fn modifier_key() -> Key {
        #[cfg(target_os = "macos")]
        {
            Key::Meta
        }
        #[cfg(not(target_os = "macos"))]
        {
            Key::Control
        }
    }

    /// Type text via clipboard (copy to clipboard, then paste)
    fn type_via_clipboard(&mut self, text: &str) -> Result<(), TypingError> {
        // Save current clipboard content (best effort)
        let old_content = self.clipboard.get_text().ok();

        // Set new content
        self.clipboard
            .set_text(text)
            .map_err(|e| TypingError::Clipboard(format!("Failed to set clipboard: {}", e)))?;

        // Small delay for clipboard to be ready
        thread::sleep(Duration::from_millis(50));

        // Send paste command
        if let Err(e) = self.send_paste() {
            eprintln!("[TYPING] Paste failed: {}", e);
            // Try to restore clipboard before returning error
            if let Some(old) = old_content {
                let _ = self.clipboard.set_text(old);
            }
            return Err(e);
        }

        // Small delay for paste to complete
        thread::sleep(Duration::from_millis(100));

        // Restore old clipboard content (best effort)
        if let Some(old) = old_content {
            let _ = self.clipboard.set_text(old);
        }

        Ok(())
    }

    /// Send paste command (Cmd+V on macOS, Ctrl+V elsewhere)
    fn send_paste(&mut self) -> Result<(), TypingError> {
        // Use Unicode 'v' which enigo should map correctly
        self.send_key_combo(&[Self::modifier_key()], Key::Unicode('v'))
    }

    /// Type text directly using enigo's text method
    fn type_direct(&mut self, text: &str) -> Result<(), TypingError> {
        self.enigo
            .text(text)
            .map_err(|e| TypingError::Enigo(format!("Failed to type text: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_method_from_str() {
        assert_eq!(InputMethod::from_str("direct"), InputMethod::Direct);
        assert_eq!(InputMethod::from_str("Direct"), InputMethod::Direct);
        assert_eq!(InputMethod::from_str("clipboard"), InputMethod::Clipboard);
        assert_eq!(InputMethod::from_str("Clipboard"), InputMethod::Clipboard);
        assert_eq!(InputMethod::from_str("unknown"), InputMethod::Direct);
    }
}
