//! Terminal UI - simple inline approach with status bar at bottom

use crate::render::UiEvent;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal;
use crossterm::{cursor, ExecutableCommand};
use std::io::{self, stdout, Write};

pub struct Tui {
    preview: String,
    status: String,
    context_words: usize,
    input: String,
    cursor_pos: usize,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self {
            preview: String::new(),
            status: "⏸ Idle".to_string(),
            context_words: 0,
            input: String::new(),
            cursor_pos: 0,
        })
    }

    pub fn restore(&self) -> io::Result<()> {
        terminal::disable_raw_mode()?;
        println!();
        Ok(())
    }

    /// Print to main terminal area (above input line)
    fn print_line(&self, text: &str) -> io::Result<()> {
        // Clear current input line, print text, redraw input
        print!("\r\x1b[K{}\n", text);
        stdout().flush()?;
        Ok(())
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) -> io::Result<()> {
        match event {
            UiEvent::Preview(text) => {
                self.preview = text;
                self.status = "🎤 Listening".to_string();
            }
            UiEvent::Final(text) => {
                self.print_line(&format!("\x1b[32m> {}\x1b[0m", text))?;
                self.preview.clear();
                self.status = "⏸ Idle".to_string();
            }
            UiEvent::Thinking => {
                self.status = "💭 Thinking".to_string();
            }
            UiEvent::Speaking => {
                self.status = "🔊 Speaking".to_string();
            }
            UiEvent::SpeakingDone => {
                self.status = "⏸ Idle".to_string();
            }
            UiEvent::ResponseChunk(text) => {
                // Print response chunks directly (cyan)
                print!("\x1b[36m{}\x1b[0m", text);
                stdout().flush()?;
                self.status = "📝 Responding".to_string();
            }
            UiEvent::ResponseEnd => {
                println!("\n");
                self.status = "⏸ Idle".to_string();
            }
            UiEvent::Idle => {
                self.status = "⏸ Idle".to_string();
                self.preview.clear();
            }
            UiEvent::Tick => {}
            UiEvent::ContextWords(count) => {
                self.context_words = count;
            }
        }
        Ok(())
    }

    /// Draw status bar and input prompt
    pub fn draw(&self) -> io::Result<()> {
        let preview_text = if self.preview.is_empty() {
            String::new()
        } else {
            format!("  \x1b[90m{}\x1b[0m", self.preview)
        };

        // Status line + input on same conceptual "bottom"
        print!(
            "\r\x1b[K\x1b[7m {} | ~{} words \x1b[0m{}\n\r\x1b[K> {}",
            self.status, self.context_words, preview_text, self.input
        );
        stdout().flush()?;
        
        // Move cursor up to input line
        stdout().execute(cursor::MoveUp(0))?;
        Ok(())
    }

    /// Poll for keyboard input, returns Some(line) if Enter pressed
    pub fn poll_input(&mut self) -> io::Result<Option<String>> {
        if !event::poll(std::time::Duration::from_millis(10))? {
            return Ok(None);
        }

        if let Event::Key(key) = event::read()? {
            // Ctrl+C to quit
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(Some("\x03".to_string()));
            }

            match key.code {
                KeyCode::Enter => {
                    let text = self.input.trim().to_string();
                    self.input.clear();
                    self.cursor_pos = 0;
                    // Clear the status+input lines before returning
                    print!("\r\x1b[K\x1b[A\r\x1b[K");
                    stdout().flush()?;
                    if !text.is_empty() {
                        return Ok(Some(text));
                    }
                }
                KeyCode::Char(c) => {
                    self.input.insert(self.cursor_pos, c);
                    self.cursor_pos += 1;
                }
                KeyCode::Backspace => {
                    if self.cursor_pos > 0 {
                        self.cursor_pos -= 1;
                        self.input.remove(self.cursor_pos);
                    }
                }
                KeyCode::Delete => {
                    if self.cursor_pos < self.input.len() {
                        self.input.remove(self.cursor_pos);
                    }
                }
                KeyCode::Left => {
                    if self.cursor_pos > 0 {
                        self.cursor_pos -= 1;
                    }
                }
                KeyCode::Right => {
                    if self.cursor_pos < self.input.len() {
                        self.cursor_pos += 1;
                    }
                }
                KeyCode::Home => {
                    self.cursor_pos = 0;
                }
                KeyCode::End => {
                    self.cursor_pos = self.input.len();
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// Set input text (for voice prefill)
    pub fn set_input(&mut self, text: &str) {
        self.input = text.to_string();
        self.cursor_pos = self.input.len();
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
