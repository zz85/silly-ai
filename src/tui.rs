//! Terminal UI with proper cursor management and synchronized updates

use crate::render::{OrbStyle, UiEvent, UiMode, UiRenderer};
use crate::state::AppMode;
use crate::status_bar::{SpinnerType, StatusBarState, StatusDisplayStyle, StatusRenderer};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, queue};
use std::fs::OpenOptions;
use std::io::{self, Write, stdout};
use unicode_width::UnicodeWidthStr;

fn debug_log(msg: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.log")
    {
        let _ = writeln!(
            file,
            "{}: {}",
            chrono::Utc::now().format("%H:%M:%S%.3f"),
            msg
        );
    }
}

pub struct Tui {
    preview: String,
    input: String,
    cursor_pos: usize,
    status_drawn: bool,
    last_drawn_lines: usize, // track how many lines were drawn
    responding: bool,
    input_activity: bool,
    keypress_activity: bool,
    status_bar: StatusBarState,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        debug_log("TUI: Creating new TUI instance");
        terminal::enable_raw_mode()?;
        execute!(stdout(), cursor::Hide)?;
        debug_log("TUI: Raw mode enabled, cursor hidden");
        let mut status_bar = StatusBarState::new();
        // Text UI always uses emoji style
        status_bar.display_style = StatusDisplayStyle::Emoji;
        Ok(Self {
            preview: String::new(),
            input: String::new(),
            cursor_pos: 0,
            status_drawn: false,
            last_drawn_lines: 0,
            responding: false,
            input_activity: false,
            keypress_activity: false,
            status_bar,
        })
    }

    pub fn set_auto_submit_progress(&mut self, progress: Option<f32>) {
        self.status_bar.auto_submit_progress = progress;
    }

    pub fn set_mic_muted(&mut self, muted: bool) {
        self.status_bar.mic_muted = muted;
    }

    pub fn set_tts_enabled(&mut self, enabled: bool) {
        self.status_bar.tts_enabled = enabled;
    }

    pub fn set_wake_enabled(&mut self, enabled: bool) {
        self.status_bar.wake_enabled = enabled;
    }

    pub fn set_mode(&mut self, mode: AppMode) {
        self.status_bar.mode = mode;
    }

    pub fn restore(&self) -> io::Result<()> {
        execute!(stdout(), cursor::Show, cursor::MoveToColumn(0))?;
        // Don't disable raw mode here - let the new UI handle terminal mode
        // or let the final cleanup disable it
        // terminal::disable_raw_mode()?;
        println!();
        Ok(())
    }

    pub fn cleanup(&self) -> io::Result<()> {
        // Final cleanup when exiting the application
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
                self.status_bar.status = "🎤 Listening".to_string();
                self.status_bar.spinner_type = SpinnerType::Bars;
            }
            UiEvent::Final(text) => {
                self.print_content(&format!("\x1b[32m>\x1b[0m {}", text))?;
                self.preview.clear();
                self.status_bar.status = "⏳ Sending".to_string();
                self.status_bar.spinner_type = SpinnerType::Dots;
            }
            UiEvent::Thinking => {
                self.status_bar.status = "💭 Thinking".to_string();
                self.status_bar.spinner_type = SpinnerType::Dots;
            }
            UiEvent::Speaking => {
                self.status_bar.status = "🔊 Speaking".to_string();
                self.status_bar.spinner_type = SpinnerType::Music;
            }
            UiEvent::SpeakingDone => {
                self.status_bar.status = "✓ Ready".to_string();
                self.status_bar.spinner_type = SpinnerType::None;
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
                self.status_bar.status = "⏸ Idle".to_string();
                self.status_bar.spinner_type = SpinnerType::None;
                self.preview.clear();
            }
            UiEvent::Tick => {}
            UiEvent::ContextWords(count) => {
                self.status_bar.context_words = count;
            }
            UiEvent::SwitchUiMode(_) => {
                // Text UI doesn't handle mode switching - this is handled in main loop
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

        // Update spinner frame
        self.status_bar.update_spinner();

        // Status line using modular status bar
        let status = self
            .status_bar
            .render_status(self.status_bar.display_style, Some(term_width));

        // Input line with optional preview and auto-submit timer
        let timer_bar = self.status_bar.auto_submit_bar();
        let prompt = if self.preview.is_empty() {
            format!("{}\x1b[32m>\x1b[0m {}", timer_bar, self.input)
        } else {
            format!(
                "\x1b[90m{}\x1b[0m {}\x1b[32m>\x1b[0m {}",
                self.preview, timer_bar, self.input
            )
        };
        let cursor_offset = if self.preview.is_empty() {
            2 + if self.status_bar.auto_submit_progress.is_some() {
                6
            } else {
                0
            } // "> " + "████⠋ "
        } else {
            self.preview.width()
                + 4
                + if self.status_bar.auto_submit_progress.is_some() {
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
        debug_log("TUI: poll_input called");
        let mut pending_submit = None;

        while event::poll(std::time::Duration::from_millis(0))? {
            debug_log("TUI: Event available");
            if let Event::Key(key) = event::read()? {
                debug_log(&format!("TUI: Key event: {:?}", key));
                self.keypress_activity = true;

                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some("\x03".to_string()));
                }
                if key.code == KeyCode::Char('m') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some("/mute".to_string()));
                }

                // 'd' key to toggle display style (emoji vs text)
                if key.code == KeyCode::Char('d') && !key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.status_bar.toggle_display_style();
                    continue;
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
        // Don't set input_activity here - this is for voice input
        // input_activity is only for keyboard input
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
        self.status_bar.status = "✓ Ready".to_string();
    }

    pub fn set_last_response_words(&mut self, words: usize) {
        self.status_bar.last_response_words = words;
    }

    pub fn set_audio_level(&mut self, level: f32) {
        self.status_bar.audio_level = level;
    }

    pub fn set_tts_level(&mut self, level: f32) {
        self.status_bar.tts_level = level;
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

// Implement StatusRenderer trait for Tui
impl StatusRenderer for Tui {
    fn update_status(&mut self, state: &StatusBarState) {
        self.status_bar = state.clone();
    }

    fn status_state(&self) -> &StatusBarState {
        &self.status_bar
    }

    fn status_state_mut(&mut self) -> &mut StatusBarState {
        &mut self.status_bar
    }

    fn preferred_display_style(&self) -> crate::status_bar::StatusDisplayStyle {
        crate::status_bar::StatusDisplayStyle::Emoji
    }
}

// Implement UiRenderer trait for Tui
impl UiRenderer for Tui {
    fn handle_ui_event(&mut self, event: UiEvent) -> io::Result<()> {
        Tui::handle_ui_event(self, event)
    }

    fn draw(&mut self) -> io::Result<()> {
        Tui::draw(self)
    }

    fn poll_input(&mut self) -> io::Result<Option<String>> {
        Tui::poll_input(self)
    }

    fn restore(&self) -> io::Result<()> {
        Tui::restore(self)
    }

    fn cleanup(&self) -> io::Result<()> {
        Tui::cleanup(self)
    }

    fn show_message(&mut self, text: &str) {
        Tui::show_message(self, text)
    }

    fn set_auto_submit_progress(&mut self, progress: Option<f32>) {
        Tui::set_auto_submit_progress(self, progress)
    }

    fn set_mic_muted(&mut self, muted: bool) {
        Tui::set_mic_muted(self, muted)
    }

    fn set_tts_enabled(&mut self, enabled: bool) {
        Tui::set_tts_enabled(self, enabled)
    }

    fn set_wake_enabled(&mut self, enabled: bool) {
        Tui::set_wake_enabled(self, enabled)
    }

    fn set_mode(&mut self, mode: AppMode) {
        Tui::set_mode(self, mode)
    }

    fn set_ready(&mut self) {
        Tui::set_ready(self)
    }

    fn set_last_response_words(&mut self, words: usize) {
        Tui::set_last_response_words(self, words)
    }

    fn set_audio_level(&mut self, level: f32) {
        Tui::set_audio_level(self, level)
    }

    fn set_tts_level(&mut self, level: f32) {
        Tui::set_tts_level(self, level)
    }

    fn has_input_activity(&mut self) -> bool {
        Tui::has_input_activity(self)
    }

    fn has_keypress_activity(&mut self) -> bool {
        Tui::has_keypress_activity(self)
    }

    fn has_pending_input(&self) -> bool {
        !self.input.trim().is_empty()
    }

    fn take_input(&mut self) -> Option<String> {
        Tui::take_input(self)
    }

    fn append_input(&mut self, text: &str) {
        Tui::append_input(self, text)
    }

    fn ui_mode(&self) -> UiMode {
        UiMode::Text
    }

    fn set_visual_style(&mut self, _style: OrbStyle) {
        // No-op for text UI
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
