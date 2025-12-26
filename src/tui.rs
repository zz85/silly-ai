//! Terminal UI with ratatui - status bar and input at bottom, chat in scrollback

use crate::render::UiEvent;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{cursor, ExecutableCommand};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use std::io::{self, stdout, Write};
use tui_textarea::{Input, TextArea};

const STATUS_HEIGHT: u16 = 1;
const INPUT_HEIGHT: u16 = 3;
const RESERVED_LINES: u16 = STATUS_HEIGHT + INPUT_HEIGHT;

pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    textarea: TextArea<'static>,
    preview: String,
    status: String,
    context_words: usize,
    needs_redraw: bool,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

        let mut textarea = TextArea::default();
        textarea.set_block(Block::default().borders(Borders::ALL).title(" Input "));
        textarea.set_cursor_line_style(Style::default());

        let mut tui = Self {
            terminal,
            textarea,
            preview: String::new(),
            status: "⏸ Idle".to_string(),
            context_words: 0,
            needs_redraw: true,
        };

        // Reserve space at bottom for status + input
        tui.reserve_bottom_space()?;

        Ok(tui)
    }

    fn reserve_bottom_space(&mut self) -> io::Result<()> {
        let size = self.terminal.size()?;
        // Move cursor to leave room at bottom
        stdout().execute(cursor::MoveTo(0, size.height.saturating_sub(RESERVED_LINES)))?;
        // Add newlines to push content up and create space
        for _ in 0..RESERVED_LINES {
            println!();
        }
        stdout().flush()?;
        Ok(())
    }

    pub fn restore(&mut self) -> io::Result<()> {
        terminal::disable_raw_mode()?;
        // Move cursor below our UI area
        let size = self.terminal.size()?;
        stdout().execute(cursor::MoveTo(0, size.height))?;
        println!();
        Ok(())
    }

    /// Print text to scrollback (chat area above status bar)
    pub fn print_to_scrollback(&mut self, text: &str) -> io::Result<()> {
        let size = self.terminal.size()?;
        let chat_bottom = size.height.saturating_sub(RESERVED_LINES);

        // Save cursor, move to chat area, print, restore
        stdout().execute(cursor::SavePosition)?;
        stdout().execute(cursor::MoveTo(0, chat_bottom))?;

        // Scroll up to make room
        stdout().execute(terminal::ScrollUp(1))?;
        stdout().execute(cursor::MoveTo(0, chat_bottom.saturating_sub(1)))?;

        // Print without newline (we scrolled already)
        print!("{}", text);
        stdout().flush()?;

        stdout().execute(cursor::RestorePosition)?;
        self.needs_redraw = true;
        Ok(())
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::Preview(text) => {
                self.preview = text;
                self.status = "🎤 Listening".to_string();
                self.needs_redraw = true;
            }
            UiEvent::Final(text) => {
                let _ = self.print_to_scrollback(&format!("> {}", text));
                self.preview.clear();
                self.status = "⏸ Idle".to_string();
            }
            UiEvent::Thinking => {
                self.status = "💭 Thinking".to_string();
                self.needs_redraw = true;
            }
            UiEvent::Speaking => {
                self.status = "🔊 Speaking".to_string();
                self.needs_redraw = true;
            }
            UiEvent::SpeakingDone => {
                self.status = "⏸ Idle".to_string();
                self.needs_redraw = true;
            }
            UiEvent::ResponseChunk(text) => {
                let _ = self.print_to_scrollback(&text);
                self.status = "📝 Responding".to_string();
            }
            UiEvent::ResponseEnd => {
                let _ = self.print_to_scrollback("\n");
                self.status = "⏸ Idle".to_string();
            }
            UiEvent::Idle => {
                self.status = "⏸ Idle".to_string();
                self.preview.clear();
                self.needs_redraw = true;
            }
            UiEvent::Tick => {}
            UiEvent::ContextWords(count) => {
                self.context_words = count;
                self.needs_redraw = true;
            }
        }
    }

    /// Poll for keyboard input, returns Some(line) if Enter pressed
    pub fn poll_input(&mut self) -> io::Result<Option<String>> {
        if event::poll(std::time::Duration::from_millis(1))? {
            if let Event::Key(key) = event::read()? {
                self.needs_redraw = true;

                // Ctrl+C to quit
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some("\x03".to_string()));
                }

                // Enter to submit
                if key.code == KeyCode::Enter {
                    let lines: Vec<String> = self.textarea.lines().iter().map(|s| s.to_string()).collect();
                    let text = lines.join("\n").trim().to_string();
                    self.textarea = TextArea::default();
                    self.textarea.set_block(Block::default().borders(Borders::ALL).title(" Input "));
                    self.textarea.set_cursor_line_style(Style::default());
                    if !text.is_empty() {
                        return Ok(Some(text));
                    }
                    return Ok(None);
                }

                // Convert crossterm key event to tui-textarea input
                let input = match key.code {
                    KeyCode::Char(c) => Input { key: tui_textarea::Key::Char(c), ctrl: key.modifiers.contains(KeyModifiers::CONTROL), alt: key.modifiers.contains(KeyModifiers::ALT), shift: key.modifiers.contains(KeyModifiers::SHIFT) },
                    KeyCode::Backspace => Input { key: tui_textarea::Key::Backspace, ctrl: false, alt: false, shift: false },
                    KeyCode::Delete => Input { key: tui_textarea::Key::Delete, ctrl: false, alt: false, shift: false },
                    KeyCode::Left => Input { key: tui_textarea::Key::Left, ctrl: false, alt: false, shift: false },
                    KeyCode::Right => Input { key: tui_textarea::Key::Right, ctrl: false, alt: false, shift: false },
                    KeyCode::Up => Input { key: tui_textarea::Key::Up, ctrl: false, alt: false, shift: false },
                    KeyCode::Down => Input { key: tui_textarea::Key::Down, ctrl: false, alt: false, shift: false },
                    KeyCode::Home => Input { key: tui_textarea::Key::Home, ctrl: false, alt: false, shift: false },
                    KeyCode::End => Input { key: tui_textarea::Key::End, ctrl: false, alt: false, shift: false },
                    _ => return Ok(None),
                };
                self.textarea.input(input);
            }
        }
        Ok(None)
    }

    /// Set input text (for voice prefill)
    pub fn set_input(&mut self, text: &str) {
        self.textarea = TextArea::from([text]);
        self.textarea.set_block(Block::default().borders(Borders::ALL).title(" Input "));
        self.textarea.set_cursor_line_style(Style::default());
        self.textarea.move_cursor(tui_textarea::CursorMove::End);
        self.needs_redraw = true;
    }

    pub fn draw(&mut self) -> io::Result<()> {
        if !self.needs_redraw {
            return Ok(());
        }
        self.needs_redraw = false;

        let preview = self.preview.clone();
        let status = format!(" {} | ~{} words ", self.status, self.context_words);
        let preview_display = if preview.is_empty() {
            " ".to_string()
        } else {
            format!(" 🎤 {}", preview)
        };

        self.terminal.draw(|frame| {
            let size = frame.area();

            // Only draw at the bottom of the screen
            let bottom_area = Rect {
                x: 0,
                y: size.height.saturating_sub(RESERVED_LINES),
                width: size.width,
                height: RESERVED_LINES,
            };

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(STATUS_HEIGHT),
                    Constraint::Length(INPUT_HEIGHT),
                ])
                .split(bottom_area);

            // Status bar with preview
            let status_with_preview = if preview.is_empty() {
                status
            } else {
                format!("{}  {}", status, preview_display)
            };
            let status_widget = Paragraph::new(status_with_preview)
                .style(Style::default().bg(Color::DarkGray).fg(Color::White));
            frame.render_widget(status_widget, chunks[0]);

            // Input area
            frame.render_widget(&self.textarea, chunks[1]);
        })?;

        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
