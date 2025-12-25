use ollama_rs::Ollama;
use ollama_rs::generation::chat::ChatMessage;
use ollama_rs::generation::chat::request::ChatMessageRequest;

const MODEL: &str = "gpt-oss:20b";

pub struct Chat {
    ollama: Ollama,
    history: Vec<ChatMessage>,
}

impl Chat {
    pub fn new() -> Self {
        Self {
            ollama: Ollama::default(),
            history: Vec::new(),
        }
    }

    pub async fn send(&mut self, message: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.history.push(ChatMessage::user(message.to_string()));

        let request = ChatMessageRequest::new(MODEL.to_string(), self.history.clone());
        let response = self.ollama.send_chat_messages(request).await?;

        let msg = response.message;
        self.history.push(msg.clone());
        Ok(msg.content)
    }
}
