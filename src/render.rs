use std::io::Write;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPEAKING: [&str; 4] = ["♪", "♫", "♪", "♬"];

#[derive(Clone)]
pub enum UiEvent {
    Preview(String),
    Final(String),
    Thinking,
    Speaking,
    SpeakingDone,
    ResponseChunk(String),
    ResponseEnd,
    Idle,
    Tick,
    ContextWords(usize),
}

#[derive(Clone)]
pub struct Ui {
    tx: flume::Sender<UiEvent>,
}

impl Ui {
    pub fn new() -> (Self, flume::Receiver<UiEvent>) {
        let (tx, rx) = flume::unbounded();
        (Self { tx }, rx)
    }

    /// Show gray preview text while user is speaking
    pub fn set_preview(&self, text: String) {
        let _ = self.tx.send(UiEvent::Preview(text));
    }

    /// Show spinner while waiting for LLM response
    pub fn set_thinking(&self) {
        let _ = self.tx.send(UiEvent::Thinking);
    }

    /// Show animation while TTS is playing audio
    pub fn set_speaking(&self) {
        let _ = self.tx.send(UiEvent::Speaking);
    }

    /// Clear status, return to idle state
    pub fn set_idle(&self) {
        let _ = self.tx.send(UiEvent::Idle);
    }

    /// Display finalized user transcription with ">" prefix
    pub fn show_final(&self, text: &str) {
        let _ = self.tx.send(UiEvent::Final(text.to_string()));
    }

    /// Append streamed LLM text (cyan colored)
    pub fn append_response(&self, text: &str) {
        let _ = self.tx.send(UiEvent::ResponseChunk(text.to_string()));
    }

    /// End LLM text stream, reset color and newline
    pub fn end_response(&self) {
        let _ = self.tx.send(UiEvent::ResponseEnd);
    }

    /// TTS audio playback finished
    pub fn speaking_done(&self) {
        let _ = self.tx.send(UiEvent::SpeakingDone);
    }

    /// Advance animation frame (call periodically)
    pub fn tick(&self) {
        let _ = self.tx.send(UiEvent::Tick);
    }

    /// Update context word count shown in status
    pub fn set_context_words(&self, count: usize) {
        let _ = self.tx.send(UiEvent::ContextWords(count));
    }
}

#[derive(Clone, PartialEq)]
enum RenderState {
    Idle,
    Preview(String),
    Thinking,
    Speaking,
    Response,
}

pub struct Renderer {
    state: RenderState,
    frame: usize,
    context_words: usize,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            state: RenderState::Idle,
            frame: 0,
            context_words: 0,
        }
    }

    pub fn handle(&mut self, event: UiEvent) {
        match event {
            UiEvent::Preview(text) => {
                let was_preview = matches!(self.state, RenderState::Preview(_));
                self.state = RenderState::Preview(text.clone());
                // Save cursor, move up (if already showing preview), clear line, print, restore
                if was_preview {
                    // Already have a preview line, just update it
                    print!("\x1b7\x1b[A\r\x1b[K\x1b[90m{}\x1b[0m\x1b8", text);
                } else {
                    // Insert new preview line above prompt
                    print!("\x1b7\x1b[A\r\x1b[K\x1b[90m{}\x1b[0m\n\x1b8", text);
                }
            }
            UiEvent::Final(text) => {
                // Clear preview line if present, then show final
                if matches!(self.state, RenderState::Preview(_)) {
                    print!("\x1b[A\r\x1b[K");
                }
                self.state = RenderState::Idle;
                print!("\r\x1b[K> {}\n", text);
            }
            UiEvent::Thinking => {
                // Clear preview line if present
                if matches!(self.state, RenderState::Preview(_)) {
                    print!("\x1b[A\r\x1b[K");
                }
                self.state = RenderState::Thinking;
                self.render_spinner();
            }
            UiEvent::Speaking => {
                self.state = RenderState::Speaking;
                self.render_speaking();
            }
            UiEvent::SpeakingDone => {
                self.state = RenderState::Idle;
                print!("\r\x1b[K");
            }
            UiEvent::ResponseChunk(text) => {
                if self.state != RenderState::Response {
                    print!("\r\x1b[K\x1b[36m");
                    self.state = RenderState::Response;
                }
                print!("{}", text);
            }
            UiEvent::ResponseEnd => {
                if self.state == RenderState::Response {
                    println!("\x1b[0m");
                }
                self.state = RenderState::Idle;
            }
            UiEvent::Idle => {
                self.state = RenderState::Idle;
                print!("\r\x1b[K");
            }
            UiEvent::Tick => {
                self.frame += 1;
                match self.state {
                    RenderState::Thinking => self.render_spinner(),
                    RenderState::Speaking => self.render_speaking(),
                    _ => return,
                }
            }
            UiEvent::ContextWords(count) => {
                self.context_words = count;
                return;
            }
        }
        std::io::stdout().flush().ok();
    }

    fn render_spinner(&self) {
        let spinner = SPINNER[self.frame % SPINNER.len()];
        print!(
            "\r\x1b[K\x1b[33m{} Thinking...\x1b[0m  \x1b[90m(~{} words)\x1b[0m",
            spinner, self.context_words
        );
    }

    fn render_speaking(&self) {
        let icon = SPEAKING[self.frame % SPEAKING.len()];
        print!(
            "\r\x1b[K\x1b[35m{} Speaking...\x1b[0m  \x1b[90m(~{} words)\x1b[0m",
            icon, self.context_words
        );
    }
}
