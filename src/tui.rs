//! Terminal UI with proper cursor management and synchronized updates

use crate::render::UiEvent;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, queue};
use std::io::{self, stdout, Write};
use unicode_width::UnicodeWidthStr;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MUSIC: [&str; 4] = ["♪", "♫", "♪", "♬"];
const BARS: [&str; 5] = ["▁", "▂", "▄", "▆", "█"];

#[derive(Clone, Copy, PartialEq)]
enum SpinnerType {
    None,
    Dots,
    Music,
    Bars,
}

pub struct Tui {
    preview: String,
    status: String,
    context_words: usize,
    last_response_words: usize,
    input: String,
    cursor_pos: usize,
    status_drawn: bool,
    responding: bool,
    ready: bool,
    spinner_type: SpinnerType,
    spin_frame: usize,
    audio_level: f32,
    input_activity: bool,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(stdout(), cursor::Hide)?;
        Ok(Self {
            preview: String::new(),
            status: "⏳ Loading".to_string(),
            context_words: 0,
            last_response_words: 0,
            input: String::new(),
            cursor_pos: 0,
            status_drawn: false,
            responding: false,
            ready: false,
            spinner_type: SpinnerType::Dots,
            spin_frame: 0,
            audio_level: 0.0,
            input_activity: false,
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
                self.spinner_type = SpinnerType::Bars;
            }
            UiEvent::Final(text) => {
                self.print_content(&format!("\x1b[32m>\x1b[0m {}", text))?;
                self.preview.clear();
                self.status = "⏳ Sending".to_string();
                self.spinner_type = SpinnerType::Dots;
            }
            UiEvent::Thinking => {
                self.status = "💭 Thinking".to_string();
                self.spinner_type = SpinnerType::Dots;
            }
            UiEvent::Speaking => {
                self.status = "🔊 Speaking".to_string();
                self.spinner_type = SpinnerType::Music;
            }
            UiEvent::SpeakingDone => {
                self.ready = true;
                self.status = "✓ Ready".to_string();
                self.spinner_type = SpinnerType::None;
            }
            UiEvent::ResponseChunk(text) => {
                if self.status_drawn {
                    let mut out = stdout();
                    queue!(out, cursor::MoveUp(1), cursor::MoveToColumn(0))?;
                    queue!(out, terminal::Clear(ClearType::FromCursorDown))?;
                    out.flush()?;
                    self.status_drawn = false;
                }
                if !self.responding {
                    println!();
                }
                self.responding = true;
                print!("{}", text);
                stdout().flush()?;
            }
            UiEvent::ResponseEnd => {
                println!("\n");
                self.status_drawn = false;
                self.responding = false;
            }
            UiEvent::Idle => {
                self.status = if self.ready { "✓ Ready".to_string() } else { "⏸ Idle".to_string() };
                self.spinner_type = SpinnerType::None;
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

        // Go to start and clear (2 lines: status + prompt)
        queue!(out, cursor::Hide)?;
        if self.status_drawn {
            queue!(out, cursor::MoveUp(1))?;
        }
        queue!(out, cursor::MoveToColumn(0), terminal::Clear(ClearType::FromCursorDown))?;

        // Status line with optional spinner (bright color)
        let spinner_str = match self.spinner_type {
            SpinnerType::None => String::new(),
            SpinnerType::Dots => {
                self.spin_frame = (self.spin_frame + 1) % SPINNER.len();
                format!("\x1b[93m{}\x1b[90m ", SPINNER[self.spin_frame])
            }
            SpinnerType::Music => {
                self.spin_frame = (self.spin_frame + 1) % MUSIC.len();
                format!("\x1b[95m{}\x1b[90m ", MUSIC[self.spin_frame])
            }
            SpinnerType::Bars => {
                // Use audio level to select bar height
                let idx = ((self.audio_level * 50.0).min(1.0) * (BARS.len() - 1) as f32) as usize;
                format!("\x1b[92m{}\x1b[90m ", BARS[idx])
            }
        };
        let status = format!(
            "\x1b[90m{}{} │ 📝 {} │ 💬 {}\x1b[0m",
            spinner_str, self.status, self.context_words, self.last_response_words
        );

        // Input line with optional preview
        let prompt = if self.preview.is_empty() {
            format!("\x1b[32m>\x1b[0m {}", self.input)
        } else {
            format!("\x1b[90m{}\x1b[0m \x1b[32m>\x1b[0m {}", self.preview, self.input)
        };
        let cursor_offset = if self.preview.is_empty() {
            2 // "> "
        } else {
            self.preview.width() + 4 // preview + " > "
        };

        queue!(out,
            crossterm::style::Print(&status),
            crossterm::style::Print("\r\n"),
            crossterm::style::Print(&prompt),
            cursor::MoveToColumn((cursor_offset + self.cursor_display_width()) as u16),
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
                    let byte_pos = self.char_to_byte_index(self.cursor_pos);
                    self.input.insert(byte_pos, c);
                    self.cursor_pos += 1;
                    self.input_activity = true;
                }
                KeyCode::Backspace if self.cursor_pos > 0 => {
                    self.cursor_pos -= 1;
                    let byte_pos = self.char_to_byte_index(self.cursor_pos);
                    self.input.remove(byte_pos);
                    self.input_activity = true;
                }
                KeyCode::Delete if self.cursor_pos < self.char_count() => {
                    let byte_pos = self.char_to_byte_index(self.cursor_pos);
                    self.input.remove(byte_pos);
                    self.input_activity = true;
                }
                KeyCode::Left => self.cursor_pos = self.cursor_pos.saturating_sub(1),
                KeyCode::Right if self.cursor_pos < self.char_count() => self.cursor_pos += 1,
                KeyCode::Home => self.cursor_pos = 0,
                KeyCode::End => self.cursor_pos = self.char_count(),
                _ => {}
            }
        }
        Ok(None)
    }

    pub fn set_input(&mut self, text: &str) {
        self.input = text.to_string();
        self.cursor_pos = self.char_count();
    }

    pub fn append_input(&mut self, text: &str) {
        if !self.input.is_empty() && !self.input.ends_with(' ') {
            self.input.push(' ');
        }
        self.input.push_str(text);
        self.cursor_pos = self.char_count();
    }

    /// Convert character index to byte index
    fn char_to_byte_index(&self, char_idx: usize) -> usize {
        self.input.char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    /// Get character count
    fn char_count(&self) -> usize {
        self.input.chars().count()
    }

    /// Get display width up to cursor position
    fn cursor_display_width(&self) -> usize {
        self.input.chars()
            .take(self.cursor_pos)
            .collect::<String>()
            .width()
    }

    pub fn set_ready(&mut self) {
        self.ready = true;
        self.status = "✓ Ready".to_string();
    }

    pub fn set_last_response_words(&mut self, words: usize) {
        self.last_response_words = words;
    }

    pub fn set_audio_level(&mut self, level: f32) {
        self.audio_level = level;
    }

    /// Check if there was input activity (keypress) since last call
    pub fn has_input_activity(&mut self) -> bool {
        let activity = self.input_activity;
        self.input_activity = false;
        activity
    }

    /// Take the current input and clear it
    pub fn take_input(&mut self) -> Option<String> {
        if self.input.is_empty() {
            None
        } else {
            let text = std::mem::take(&mut self.input);
            self.cursor_pos = 0;
            Some(text)
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
