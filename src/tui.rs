//! Terminal UI with proper cursor management and synchronized updates

use crate::render::UiEvent;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, queue};
use std::io::{self, stdout, Write};

pub struct Tui {
    preview: String,
    status: String,
    context_words: usize,
    input: String,
    cursor_pos: usize,
    last_status_lines: u16,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        // Hide cursor during setup
        execute!(stdout(), cursor::Hide)?;
        Ok(Self {
            preview: String::new(),
            status: "⏸ Idle".to_string(),
            context_words: 0,
            input: String::new(),
            cursor_pos: 0,
            last_status_lines: 0,
        })
    }

    pub fn restore(&self) -> io::Result<()> {
        execute!(
            stdout(),
            cursor::Show,
            cursor::MoveToColumn(0),
        )?;
        terminal::disable_raw_mode()?;
        println!();
        Ok(())
    }

    /// Clear the status area and move cursor to start of output area
    fn clear_status_area(&mut self) -> io::Result<()> {
        let mut out = stdout();
        if self.last_status_lines > 0 {
            // Move up and clear each line we previously drew
            for _ in 0..self.last_status_lines {
                queue!(out, cursor::MoveUp(1), terminal::Clear(ClearType::CurrentLine))?;
            }
        }
        out.flush()
    }

    /// Print to main terminal area (scrolling content)
    fn print_line(&mut self, text: &str) -> io::Result<()> {
        self.clear_status_area()?;
        println!("{}", text);
        self.last_status_lines = 0;
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
                self.clear_status_area()?;
                print!("\x1b[36m{}\x1b[0m", text);
                stdout().flush()?;
                self.last_status_lines = 0;
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

    /// Draw status bar and input prompt at bottom
    pub fn draw(&mut self) -> io::Result<()> {
        let mut out = stdout();

        // Clear previous status area
        self.clear_status_area()?;

        // Build status line
        let status_line = format!(
            "\x1b[7m {} | ~{} words \x1b[0m",
            self.status, self.context_words
        );

        // Build preview (if any)
        let preview_part = if self.preview.is_empty() {
            String::new()
        } else {
            format!("  \x1b[90m{}\x1b[0m", self.preview)
        };

        // Build input line
        let input_line = format!("> {}", self.input);

        // Draw: status + preview on line 1, input on line 2
        queue!(
            out,
            cursor::Hide,
            crossterm::style::Print(&status_line),
            crossterm::style::Print(&preview_part),
            crossterm::style::Print("\n"),
            crossterm::style::Print(&input_line),
        )?;

        // Position cursor within input
        let cursor_col = 2 + self.cursor_pos as u16; // "> " prefix
        queue!(
            out,
            cursor::MoveToColumn(cursor_col),
            cursor::Show,
        )?;

        out.flush()?;
        self.last_status_lines = 2;
        Ok(())
    }

    /// Poll for keyboard input, returns Some(line) if Enter pressed
    pub fn poll_input(&mut self) -> io::Result<Option<String>> {
        if !event::poll(std::time::Duration::from_millis(10))? {
            return Ok(None);
        }

        if let Event::Key(key) = event::read()? {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(Some("\x03".to_string()));
            }

            match key.code {
                KeyCode::Enter => {
                    let text = self.input.trim().to_string();
                    self.input.clear();
                    self.cursor_pos = 0;
                    self.clear_status_area()?;
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
                    self.cursor_pos = self.cursor_pos.saturating_sub(1);
                }
                KeyCode::Right => {
                    if self.cursor_pos < self.input.len() {
                        self.cursor_pos += 1;
                    }
                }
                KeyCode::Home => self.cursor_pos = 0,
                KeyCode::End => self.cursor_pos = self.input.len(),
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
