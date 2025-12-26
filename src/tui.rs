//! Terminal UI with ratatui - split screen for preview, response, and input

use crate::render::UiEvent;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use std::io::{self, stdout};
use tui_textarea::{Input, TextArea};

pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    textarea: TextArea<'static>,
    preview: String,
    status: String,
    response: String,
    context_words: usize,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

        let mut textarea = TextArea::default();
        textarea.set_block(Block::default().borders(Borders::ALL).title(" Input "));
        textarea.set_cursor_line_style(Style::default());

        Ok(Self {
            terminal,
            textarea,
            preview: String::new(),
            status: "⏸ Idle".to_string(),
            response: String::new(),
            context_words: 0,
        })
    }

    pub fn restore(&mut self) -> io::Result<()> {
        terminal::disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::Preview(text) => {
                self.preview = text;
                self.status = "🎤 Listening".to_string();
            }
            UiEvent::Final(text) => {
                self.response.push_str(&format!("> {}\n", text));
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
                self.response.push_str(&text);
                self.status = "📝 Responding".to_string();
            }
            UiEvent::ResponseEnd => {
                self.response.push_str("\n\n");
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
    }

    /// Poll for keyboard input, returns Some(line) if Enter pressed
    pub fn poll_input(&mut self) -> io::Result<Option<String>> {
        if event::poll(std::time::Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                // Ctrl+C to quit
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(Some("\x03".to_string())); // Signal quit
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
    }

    pub fn draw(&mut self) -> io::Result<()> {
        let preview = self.preview.clone();
        let status = self.status.clone();
        let response = self.response.clone();
        let context_words = self.context_words;

        self.terminal.draw(|frame| {
            let area = frame.area();

            // Layout: response area (flex), preview (1 line), status bar (1 line), input (3 lines)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),      // Response
                    Constraint::Length(3),   // Preview
                    Constraint::Length(1),   // Status
                    Constraint::Length(3),   // Input
                ])
                .split(area);

            // Response area with scroll to bottom
            let response_lines: Vec<&str> = response.lines().collect();
            let visible_height = chunks[0].height.saturating_sub(2) as usize;
            let scroll = response_lines.len().saturating_sub(visible_height);
            let response_widget = Paragraph::new(response.as_str())
                .block(Block::default().borders(Borders::ALL).title(" Chat "))
                .wrap(Wrap { trim: false })
                .scroll((scroll as u16, 0))
                .style(Style::default().fg(Color::Cyan));
            frame.render_widget(response_widget, chunks[0]);

            // Preview area
            let preview_widget = Paragraph::new(preview.as_str())
                .block(Block::default().borders(Borders::ALL).title(" Preview "))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(preview_widget, chunks[1]);

            // Status bar
            let status_text = format!(" {} | ~{} words ", status, context_words);
            let status_widget = Paragraph::new(status_text)
                .style(Style::default().bg(Color::DarkGray).fg(Color::White));
            frame.render_widget(status_widget, chunks[2]);

            // Input area
            frame.render_widget(&self.textarea, chunks[3]);
        })?;

        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
