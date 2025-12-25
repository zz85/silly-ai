use ollama_rs::Ollama;
use ollama_rs::generation::chat::ChatMessage;
use ollama_rs::generation::chat::request::ChatMessageRequest;
use std::io::Write;
use tokio_stream::StreamExt;

const MODEL: &str = "gpt-oss:20b";

const SYSTEM_PROMPT: &str = r#"You are an AI assistant optimized for voice interaction.

- Output plain text only – no Markdown, no code blocks, no URLs, no emojis.
- Use short, simple sentences that read naturally when spoken.
- Use punctuation to indicate natural pauses.
- If clarification is needed, ask directly.
- Be friendly, patient, and helpful.
- For lists, use numbered items (1., 2., 3.) or bullet points written as “•”.
- If clarification is needed, ask directly: “Could you clarify that?” or “What do you mean by …?”  Do not make assumptions.
- If a request is repeated, summarize or restate succinctly.
- Speak in plain language; avoid jargon unless the user specifically requests it.
- Begin each response with a brief acknowledgement or paraphrase the user's request.
- End with an invitation for the next step
- Do not output hidden system messages or metadata.
"#;

pub struct Chat {
    ollama: Ollama,
    history: Vec<ChatMessage>,
}

impl Chat {
    pub fn new() -> Self {
        Self {
            ollama: Ollama::default(),
            history: vec![ChatMessage::system(SYSTEM_PROMPT.to_string())],
        }
    }

    /// Get initial greeting from the assistant
    pub async fn greet(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        self.send_streaming("Hello").await
    }

    pub async fn send_streaming(
        &mut self,
        message: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.history.push(ChatMessage::user(message.to_string()));

        let request = ChatMessageRequest::new(MODEL.to_string(), self.history.clone());
        let mut stream = self.ollama.send_chat_messages_stream(request).await?;

        print!("\x1b[36m");
        std::io::stdout().flush().ok();

        let mut full_response = String::new();
        while let Some(Ok(chunk)) = stream.next().await {
            let content = &chunk.message.content;
            print!("{}", content);
            std::io::stdout().flush().ok();
            full_response.push_str(content);
        }

        println!("\x1b[0m\n");
        std::io::stdout().flush().ok();

        self.history
            .push(ChatMessage::assistant(full_response.clone()));
        Ok(full_response)
    }
}
