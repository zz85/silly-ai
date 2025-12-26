//! UI event types and sender for cross-thread communication

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

    pub fn set_preview(&self, text: String) {
        let _ = self.tx.send(UiEvent::Preview(text));
    }

    pub fn set_thinking(&self) {
        let _ = self.tx.send(UiEvent::Thinking);
    }

    pub fn set_speaking(&self) {
        let _ = self.tx.send(UiEvent::Speaking);
    }

    pub fn set_idle(&self) {
        let _ = self.tx.send(UiEvent::Idle);
    }

    pub fn show_final(&self, text: &str) {
        let _ = self.tx.send(UiEvent::Final(text.to_string()));
    }

    pub fn append_response(&self, text: &str) {
        let _ = self.tx.send(UiEvent::ResponseChunk(text.to_string()));
    }

    pub fn end_response(&self) {
        let _ = self.tx.send(UiEvent::ResponseEnd);
    }

    pub fn speaking_done(&self) {
        let _ = self.tx.send(UiEvent::SpeakingDone);
    }

    pub fn tick(&self) {
        let _ = self.tx.send(UiEvent::Tick);
    }

    pub fn set_context_words(&self, count: usize) {
        let _ = self.tx.send(UiEvent::ContextWords(count));
    }
}
