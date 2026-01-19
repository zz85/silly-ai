//! Terminal UI with proper cursor management and synchronized updates

use crate::render::UiEvent;
use crate::state::AppMode;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, queue};
use std::io::{self, Write, stdout};
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
    last_drawn_lines: usize, // track how many lines were drawn
    responding: bool,
    ready: bool,
    spinner_type: SpinnerType,
    spin_frame: usize,
    audio_level: f32,
    input_activity: bool,
    keypress_activity: bool,
    mic_muted: bool,
    tts_enabled: bool,
    wake_enabled: bool,
    auto_submit_progress: Option<f32>, // 1.0 = full, 0.0 = about to submit
    mode: AppMode,
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
            last_drawn_lines: 0,
            responding: false,
            ready: false,
            spinner_type: SpinnerType::Dots,
            spin_frame: 0,
            audio_level: 0.0,
            input_activity: false,
            keypress_activity: false,
            mic_muted: false,
            tts_enabled: true,
            wake_enabled: true,
            auto_submit_progress: None,
            mode: AppMode::Idle,
        })
    }

    pub fn set_auto_submit_progress(&mut self, progress: Option<f32>) {
        self.auto_submit_progress = progress;
    }

    pub fn set_mic_muted(&mut self, muted: bool) {
        self.mic_muted = muted;
    }

    pub fn set_tts_enabled(&mut self, enabled: bool) {
        self.tts_enabled = enabled;
    }

    pub fn set_wake_enabled(&mut self, enabled: bool) {
        self.wake_enabled = enabled;
    }

    pub fn set_mode(&mut self, mode: AppMode) {
        self.mode = mode;
    }

    pub fn restore(&self) -> io::Result<()> {
        execute!(stdout(), cursor::Show, cursor::MoveToColumn(0))?;
        terminal::disable_raw_mode()?;
        println!();
        Ok(())
    }

    /// Move cursor to start of status area
    #[allow(dead_code)]
    fn goto_status_start(&self) -> io::Result<()> {
        let mut out = stdout();
        if self.status_drawn && self.last_drawn_lines > 0 {
            queue!(
                out,
                cursor::MoveUp(self.last_drawn_lines as u16),
                cursor::MoveToColumn(0)
            )?;
        }
        out.flush()
    }

    /// Print scrolling content (clears status, prints, marks status not drawn)
    fn print_content(&mut self, text: &str) -> io::Result<()> {
        let mut out = stdout();
        if self.status_drawn && self.last_drawn_lines > 0 {
            queue!(
                out,
                cursor::MoveUp(self.last_drawn_lines as u16),
                cursor::MoveToColumn(0)
            )?;
            queue!(out, terminal::Clear(ClearType::FromCursorDown))?;
        }
        queue!(
            out,
            crossterm::style::Print(text),
            crossterm::style::Print("\r\n")
        )?;
        out.flush()?;
        self.status_drawn = false;
        self.last_drawn_lines = 0;
        Ok(())
    }

    /// Show a multi-line message (e.g., stats output)
    pub fn show_message(&mut self, text: &str) {
        for line in text.lines() {
            let _ = self.print_content(line);
        }
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
                self.status = if self.ready {
                    "✓ Ready".to_string()
                } else {
                    "⏸ Idle".to_string()
                };
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
        let term_width = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);

        // Go to start and clear
        queue!(out, cursor::Hide)?;
        if self.status_drawn && self.last_drawn_lines > 0 {
            queue!(out, cursor::MoveUp(self.last_drawn_lines as u16))?;
        }
        queue!(
            out,
            cursor::MoveToColumn(0),
            terminal::Clear(ClearType::FromCursorDown)
        )?;

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
        let toggles = format!(
            "{}{}{}",
            if self.mic_muted { "🔇" } else { "🎙" },
            if self.tts_enabled { "🔊" } else { "🔈" },
            if self.wake_enabled { "👂" } else { "💤" },
        );
        // Mode indicator with color coding
        let mode_str = match self.mode {
            AppMode::Idle => "⏸ Idle",
            AppMode::Chat => "\x1b[92m💬 Chat\x1b[90m",
            AppMode::Transcribe => "\x1b[93m📝 Transcribe\x1b[90m",
            AppMode::NoteTaking => "\x1b[95m📓 Note\x1b[90m",
            AppMode::Command => "\x1b[96m⌘ Command\x1b[90m",
        };
        let status_content = format!(
            "{}{} │ {} │ {} │ 📝 {} │ 💬 {}",
            spinner_str, self.status, mode_str, toggles, self.context_words, self.last_response_words
        );
        let status_width = status_content.width();
        let padding = if term_width > status_width {
            (term_width - status_width) / 2
        } else {
            0
        };
        let status = format!("\x1b[90m{}{}\x1b[0m", " ".repeat(padding), status_content);

        // Input line with optional preview and auto-submit timer
        let timer_bar = if let Some(progress) = self.auto_submit_progress {
            const BLOCKS: &[char] = &[' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
            let total_steps = 4 * 8;
            let step = (progress * total_steps as f32) as usize;
            let full = step / 8;
            let partial = step % 8;
            let mut bar = "█".repeat(full);
            if full < 4 {
                bar.push(BLOCKS[partial]);
                bar.push_str(&" ".repeat(3 - full));
            }
            self.spin_frame = self.spin_frame.wrapping_add(1);
            let spinner = SPINNER[self.spin_frame % SPINNER.len()];
            format!("\x1b[33m{}{}\x1b[0m ", bar, spinner)
        } else {
            String::new()
        };
        let prompt = if self.preview.is_empty() {
            format!("{}\x1b[32m>\x1b[0m {}", timer_bar, self.input)
        } else {
            format!(
                "\x1b[90m{}\x1b[0m {}\x1b[32m>\x1b[0m {}",
                self.preview, timer_bar, self.input
            )
        };
        let cursor_offset = if self.preview.is_empty() {
            2 + if self.auto_submit_progress.is_some() {
                6
            } else {
                0
            } // "> " + "████⠋ "
        } else {
            self.preview.width()
                + 4
                + if self.auto_submit_progress.is_some() {
                    6
                } else {
                    0
                }
        };

        // Calculate how many lines the prompt takes (visible width, not including ANSI codes)
        let prompt_visible_width = cursor_offset + self.input.width();
        let prompt_lines = if term_width > 0 && prompt_visible_width > 0 {
            (prompt_visible_width + term_width - 1) / term_width
        } else {
            1
        };
        // Cursor is at end of input, need to go up: (prompt_lines - 1) to get to first prompt line, +1 for status
        self.last_drawn_lines = prompt_lines; // lines below status line

        queue!(
            out,
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
        let mut pending_submit = None;

        while event::poll(std::time::Duration::from_millis(0))? {
            if let Event::Key(key) = event::read()? {
                self.keypress_activity = true;

                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some("\x03".to_string()));
                }
                if key.code == KeyCode::Char('m') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some("/mute".to_string()));
                }

                match key.code {
                    KeyCode::Enter => {
                        if event::poll(std::time::Duration::from_millis(0))? {
                            // More events pending - this Enter is part of paste, insert newline
                            let byte_pos = self.char_to_byte_index(self.cursor_pos);
                            self.input.insert(byte_pos, '\n');
                            self.cursor_pos += 1;
                            self.input_activity = true;
                            pending_submit = None; // Clear any pending submit
                        } else {
                            // No more events yet - queue submit
                            let text = self.input.trim().to_string();
                            self.input.clear();
                            self.cursor_pos = 0;
                            pending_submit = if !text.is_empty() { Some(text) } else { None };
                        }
                    }
                    KeyCode::Char(c) => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            match c {
                                'a' => self.cursor_pos = 0,
                                'e' => self.cursor_pos = self.char_count(),
                                'k' => {
                                    if self.cursor_pos < self.char_count() {
                                        let byte_pos = self.char_to_byte_index(self.cursor_pos);
                                        self.input.truncate(byte_pos);
                                        self.input_activity = true;
                                    }
                                }
                                'u' => {
                                    if self.cursor_pos > 0 {
                                        let byte_pos = self.char_to_byte_index(self.cursor_pos);
                                        self.input = self.input[byte_pos..].to_string();
                                        self.cursor_pos = 0;
                                        self.input_activity = true;
                                    }
                                }
                                'w' => {
                                    if self.cursor_pos > 0 {
                                        let chars: Vec<char> = self.input.chars().collect();
                                        let mut end = self.cursor_pos;

                                        while end > 0 && chars[end - 1].is_whitespace() {
                                            end -= 1;
                                        }
                                        while end > 0 && !chars[end - 1].is_whitespace() {
                                            end -= 1;
                                        }

                                        let start_byte = self.char_to_byte_index(end);
                                        let end_byte = self.char_to_byte_index(self.cursor_pos);
                                        self.input.replace_range(start_byte..end_byte, "");
                                        self.cursor_pos = end;
                                        self.input_activity = true;
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            let byte_pos = self.char_to_byte_index(self.cursor_pos);
                            self.input.insert(byte_pos, c);
                            self.cursor_pos += 1;
                            self.input_activity = true;
                        }
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
        }

        Ok(pending_submit)
    }

    #[allow(dead_code)]
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
        self.input
            .char_indices()
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
        self.input
            .chars()
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

    /// Check if there was any keypress since last call
    pub fn has_keypress_activity(&mut self) -> bool {
        let activity = self.keypress_activity;
        self.keypress_activity = false;
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
