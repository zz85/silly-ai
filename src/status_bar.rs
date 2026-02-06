//! Modular status bar for both text and graphical UI modes

use crate::state::AppMode;
use unicode_width::UnicodeWidthStr;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MUSIC: [&str; 4] = ["♪", "♫", "♪", "♬"];
const BARS: [&str; 5] = ["▁", "▂", "▄", "▆", "█"];

#[derive(Clone, Copy, PartialEq)]
pub enum SpinnerType {
    None,
    Dots,
    Music,
    Bars,
}

#[derive(Clone, Copy, PartialEq)]
pub enum StatusDisplayStyle {
    /// Use emojis and symbols (text mode)
    Emoji,
    /// Use text labels (graphical mode)
    Text,
}

impl StatusDisplayStyle {
    pub fn next(&self) -> Self {
        match self {
            StatusDisplayStyle::Emoji => StatusDisplayStyle::Text,
            StatusDisplayStyle::Text => StatusDisplayStyle::Emoji,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            StatusDisplayStyle::Emoji => "Emoji",
            StatusDisplayStyle::Text => "Text",
        }
    }
}

#[derive(Clone)]
pub struct StatusBarState {
    pub status: String,
    pub spinner_type: SpinnerType,
    pub spin_frame: usize,
    pub audio_level: f32,
    pub tts_level: f32,
    pub mic_muted: bool,
    pub tts_enabled: bool,
    pub wake_enabled: bool,
    pub mode: AppMode,
    pub context_words: usize,
    pub last_response_words: usize,
    pub auto_submit_progress: Option<f32>,
    pub display_style: StatusDisplayStyle,
}

impl Default for StatusBarState {
    fn default() -> Self {
        Self {
            status: "Loading...".to_string(),
            spinner_type: SpinnerType::Dots,
            spin_frame: 0,
            audio_level: 0.0,
            tts_level: 0.0,
            mic_muted: false,
            tts_enabled: true,
            wake_enabled: true,
            mode: AppMode::Chat,
            context_words: 0,
            last_response_words: 0,
            auto_submit_progress: None,
            display_style: StatusDisplayStyle::Emoji,
        }
    }
}

impl StatusBarState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_spinner(&mut self) {
        self.spin_frame = self.spin_frame.wrapping_add(1);
    }

    pub fn toggle_display_style(&mut self) {
        self.display_style = self.display_style.next();
    }

    /// Generate the spinner string based on current state
    pub fn spinner_string(&self) -> String {
        match self.spinner_type {
            SpinnerType::None => String::new(),
            SpinnerType::Dots => {
                let frame = self.spin_frame % SPINNER.len();
                format!("\x1b[93m{}\x1b[90m ", SPINNER[frame])
            }
            SpinnerType::Music => {
                let frame = self.spin_frame % MUSIC.len();
                format!("\x1b[95m{}\x1b[90m ", MUSIC[frame])
            }
            SpinnerType::Bars => {
                let idx = ((self.audio_level * 50.0).min(1.0) * (BARS.len() - 1) as f32) as usize;
                format!("\x1b[92m{}\x1b[90m ", BARS[idx])
            }
        }
    }

    /// Generate the mode string with color coding
    pub fn mode_string(&self) -> &'static str {
        match self.mode {
            AppMode::Chat => "\x1b[92m💬 Chat\x1b[90m",
            AppMode::Paused => "\x1b[33m⏸ Paused\x1b[90m",
            AppMode::Transcribe => "\x1b[93m📝 Transcribe\x1b[90m",
            AppMode::NoteTaking => "\x1b[95m📓 Note\x1b[90m",
            AppMode::Command => "\x1b[96m⌘ Command\x1b[90m",
            AppMode::Typing => "\x1b[94m⌨ Typing\x1b[90m",
        }
    }

    /// Generate the toggles string (mic, tts, wake) with configurable style
    pub fn toggles_string(&self, style: StatusDisplayStyle) -> String {
        match style {
            StatusDisplayStyle::Emoji => format!(
                "{}{}{}",
                if self.mic_muted { "🔇" } else { "🎙" },
                if self.tts_enabled { "🔊" } else { "🔈" },
                if self.wake_enabled { "👂" } else { "💤" },
            ),
            StatusDisplayStyle::Text => format!(
                "{}{}{}",
                if self.mic_muted {
                    "\x1b[31m[MIC OFF]\x1b[0m"
                } else {
                    "\x1b[32m[MIC]\x1b[0m"
                },
                if self.tts_enabled {
                    "\x1b[32m[TTS]\x1b[0m"
                } else {
                    "\x1b[31m[TTS OFF]\x1b[0m"
                },
                if self.wake_enabled {
                    "\x1b[32m[WAKE]\x1b[0m"
                } else {
                    "\x1b[33m[NO WAKE]\x1b[0m"
                },
            ),
        }
    }

    /// Generate the TTS visualization string
    pub fn tts_viz_string(&self) -> String {
        if self.spinner_type == SpinnerType::Music && self.tts_level > 0.0 {
            let idx = ((self.tts_level * 30.0).min(1.0) * (BARS.len() - 1) as f32) as usize;
            format!(" │ \x1b[95m{}\x1b[90m", BARS[idx])
        } else {
            String::new()
        }
    }

    /// Generate the auto-submit progress bar
    pub fn auto_submit_bar(&self) -> String {
        if let Some(progress) = self.auto_submit_progress {
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
            let spinner = SPINNER[self.spin_frame % SPINNER.len()];
            format!("\x1b[33m{}{}\x1b[0m ", bar, spinner)
        } else {
            String::new()
        }
    }

    /// Generate a unified status line with configurable style
    pub fn render_status(&self, style: StatusDisplayStyle, term_width: Option<usize>) -> String {
        let spinner_str = self.spinner_string();
        let toggles = self.toggles_string(style);
        let tts_viz = self.tts_viz_string();
        let mode_str = self.mode_string();

        let status_content = match style {
            StatusDisplayStyle::Emoji => format!(
                "{}{} │ {} │ {}{} │ 📝 {} │ 💬 {}",
                spinner_str,
                self.status,
                mode_str,
                toggles,
                tts_viz,
                self.context_words,
                self.last_response_words
            ),
            StatusDisplayStyle::Text => format!(
                " \x1b[1m{}\x1b[0m | {} | {} | Ctx: {} | Resp: {}",
                self.status, mode_str, toggles, self.context_words, self.last_response_words
            ),
        };

        // Apply centering for text mode if terminal width is provided
        if let (Some(width), StatusDisplayStyle::Emoji) = (term_width, style) {
            let status_width = status_content.width();
            let padding = if width > status_width {
                (width - status_width) / 2
            } else {
                0
            };
            format!("\x1b[90m{}{}\x1b[0m", " ".repeat(padding), status_content)
        } else {
            status_content
        }
    }

    /// Generate the complete status line for text mode (backward compatibility)
    pub fn render_text_status(&self, term_width: usize) -> String {
        self.render_status(StatusDisplayStyle::Emoji, Some(term_width))
    }

    /// Generate the complete status line for graphical mode (backward compatibility)
    pub fn render_graphical_status(&self) -> String {
        self.render_status(StatusDisplayStyle::Text, None)
    }
}

/// Trait for components that can render status information
pub trait StatusRenderer {
    /// Update the status bar state
    fn update_status(&mut self, state: &StatusBarState);

    /// Get the current status bar state
    fn status_state(&self) -> &StatusBarState;

    /// Get a mutable reference to the status bar state
    fn status_state_mut(&mut self) -> &mut StatusBarState;

    /// Get the preferred display style for this renderer
    fn preferred_display_style(&self) -> StatusDisplayStyle {
        StatusDisplayStyle::Emoji // Default to emoji style
    }
}
