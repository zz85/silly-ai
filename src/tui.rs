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
    status_drawn: bool,
    responding: bool,
    ready: bool,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), cursor::Hide)?;
        Ok(Self {
            preview: String::new(),
            status: "⏳ Loading".to_string(),
            context_words: 0,
            input: String::new(),
            cursor_pos: 0,
            status_drawn: false,
            responding: false,
            ready: false,
        })
    }

    pub fn restore(&self) -> io::Result<()> {
        execute!(stdout(), cursor::Show, cursor::MoveToColumn(0))?;
        terminal::disable_raw_mode()?;
        println!();
        Ok(())
    }

    /// Move cursor to start of status area (2 lines: status + input)
    fn goto_status_start(&self) -> io::Result<()> {
        let mut out = stdout();
        if self.status_drawn {
            // We're at end of input line, go up 1 to status line, column 0
            queue!(out, cursor::MoveUp(1), cursor::MoveToColumn(0))?;
        }
        out.flush()
    }

    /// Print scrolling content (clears status, prints, marks status not drawn)
    fn print_content(&mut self, text: &str) -> io::Result<()> {
        let mut out = stdout();
        if self.status_drawn {
            queue!(out, cursor::MoveUp(1), cursor::MoveToColumn(0))?;
            queue!(out, terminal::Clear(ClearType::FromCursorDown))?;
        }
        queue!(out, crossterm::style::Print(text), crossterm::style::Print("\r\n"))?;
        out.flush()?;
        self.status_drawn = false;
        Ok(())
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) -> io::Result<()> {
        match event {
            UiEvent::Preview(text) => {
                self.preview = text;
                self.status = "🎤 Listening".to_string();
            }
            UiEvent::Final(text) => {
                self.print_content(&format!("\x1b[32m> {}\x1b[0m", text))?;
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
                self.ready = true;
                self.status = "✓ Ready".to_string();
            }
            UiEvent::ResponseChunk(text) => {
                // Clear status area once, then stream inline
                if self.status_drawn {
                    let mut out = stdout();
                    queue!(out, cursor::MoveUp(1), cursor::MoveToColumn(0))?;
                    queue!(out, terminal::Clear(ClearType::FromCursorDown))?;
                    out.flush()?;
                    self.status_drawn = false;
                }
                self.responding = true;
                print!("\x1b[36m{}\x1b[0m", text);
                stdout().flush()?;
            }
            UiEvent::ResponseEnd => {
                println!("\x1b[0m");
                self.status = "⏸ Idle".to_string();
                self.status_drawn = false;
                self.responding = false;
            }
            UiEvent::Idle => {
                self.status = if self.ready { "✓ Ready".to_string() } else { "⏸ Idle".to_string() };
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
    pub fn draw(&mut self) -> io::Result<()> {
        // Skip drawing during response streaming
        if self.responding {
            return Ok(());
        }

        let mut out = stdout();

        // Go to status start and clear
        queue!(out, cursor::Hide)?;
        if self.status_drawn {
            queue!(out, cursor::MoveUp(1))?;
        }
        queue!(out, cursor::MoveToColumn(0), terminal::Clear(ClearType::FromCursorDown))?;

        // Status line
        let status = format!("\x1b[7m {} | ~{} words \x1b[0m", self.status, self.context_words);

        // Input line with optional preview
        let prompt = if self.preview.is_empty() {
            format!("> {}", self.input)
        } else {
            format!("\x1b[90m{}\x1b[0m > {}", self.preview, self.input)
        };
        let cursor_offset = if self.preview.is_empty() {
            2 // "> "
        } else {
            self.preview.len() + 4 // preview + " > "
        };

        queue!(out,
            crossterm::style::Print(&status),
            crossterm::style::Print("\r\n"),
            crossterm::style::Print(&prompt),
            cursor::MoveToColumn((cursor_offset + self.cursor_pos) as u16),
            cursor::Show,
        )?;
        out.flush()?;
        self.status_drawn = true;
        Ok(())
    }

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
                    if !text.is_empty() {
                        return Ok(Some(text));
                    }
                }
                KeyCode::Char(c) => {
                    self.input.insert(self.cursor_pos, c);
                    self.cursor_pos += 1;
                }
                KeyCode::Backspace if self.cursor_pos > 0 => {
                    self.cursor_pos -= 1;
                    self.input.remove(self.cursor_pos);
                }
                KeyCode::Delete if self.cursor_pos < self.input.len() => {
                    self.input.remove(self.cursor_pos);
                }
                KeyCode::Left => self.cursor_pos = self.cursor_pos.saturating_sub(1),
                KeyCode::Right if self.cursor_pos < self.input.len() => self.cursor_pos += 1,
                KeyCode::Home => self.cursor_pos = 0,
                KeyCode::End => self.cursor_pos = self.input.len(),
                _ => {}
            }
        }
        Ok(None)
    }

    pub fn set_input(&mut self, text: &str) {
        self.input = text.to_string();
        self.cursor_pos = self.input.len();
    }

    pub fn set_ready(&mut self) {
        self.ready = true;
        self.status = "✓ Ready".to_string();
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
