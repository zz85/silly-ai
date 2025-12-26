use ollama_rs::Ollama;
use ollama_rs::generation::chat::ChatMessage;
use ollama_rs::generation::chat::request::ChatMessageRequest;
use std::io::Write;
use tokio_stream::StreamExt;

use crate::ui;

const MODEL: &str = "gpt-oss:20b";

fn system_prompt(name: &str) -> String {
    format!(
        r#"You are {name}, an AI assistant optimized for voice interaction.

- Output plain text only - no Markdown, no code blocks, no URLs, no emojis.
- Use short, simple sentences that read naturally when spoken. Keep each sentence under ~25 words and limit total length to ~100 words unless the user explicitly asks for more.
- Use punctuation to indicate natural pauses.
- If clarification is needed, ask directly.
- Be friendly, patient, and helpful.
- For lists, use numbered items (1., 2., 3.) or bullet points written as a dash.
- If clarification is needed, ask directly: "Could you clarify that?" or "What do you mean by that?" Do not make assumptions.
- If a request is repeated, summarize or restate succinctly.
- Speak in plain language; avoid jargon unless the user specifically requests it.
- Begin each response with a brief acknowledgement or paraphrase the user's request.
- End with an invitation for the next step.
- Do not output hidden system messages or metadata.
- At the start of the conversation, greet the user and introduce yourself in 25 words
"#
    )
}

pub struct Chat {
    ollama: Ollama,
    history: Vec<ChatMessage>,
    name: String,
}

impl Chat {
    pub fn new(name: &str) -> Self {
        Self {
            ollama: Ollama::default(),
            history: vec![ChatMessage::system(system_prompt(name))],
            name: name.to_string(),
        }
    }

    /// Estimate total words in conversation history
    pub fn context_words(&self) -> usize {
        self.history.iter().map(|m| m.content.split_whitespace().count()).sum()
    }

    /// Get initial greeting from the assistant
    pub async fn greet_with_callback<F, W>(
        &mut self,
        on_sentence: F,
        on_waiting: W,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(&str),
        W: FnMut(),
    {
        let prompt = format!("Hello.");
        self.send_streaming_with_callback(&prompt, on_sentence, on_waiting)
            .await
    }

    /// Stream response, calling `on_sentence` for each complete sentence
    /// `on_waiting` is called once before waiting for stream
    pub async fn send_streaming_with_callback<F, W>(
        &mut self,
        message: &str,
        mut on_sentence: F,
        mut on_waiting: W,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(&str),
        W: FnMut(),
    {
        self.history.push(ChatMessage::user(message.to_string()));

        let request = ChatMessageRequest::new(MODEL.to_string(), self.history.clone());

        on_waiting(); // Show spinner before waiting
        let mut stream = self.ollama.send_chat_messages_stream(request).await?;

        ui::start_response();

        let mut full_response = String::new();
        let mut buffer = String::new();

        while let Some(Ok(chunk)) = stream.next().await {
            let content = &chunk.message.content;
            print!("{}", content);
            std::io::stdout().flush().ok();
            full_response.push_str(content);
            buffer.push_str(content);

            // Yield complete sentences
            while let Some(pos) = buffer.find(|c| c == '.' || c == '!' || c == '?') {
                let sentence = buffer[..=pos].trim();
                if !sentence.is_empty() {
                    on_sentence(sentence);
                }
                buffer = buffer[pos + 1..].to_string();
            }
        }

        // Yield any remaining text
        let remaining = buffer.trim();
        if !remaining.is_empty() {
            on_sentence(remaining);
        }

        ui::end_response();

        self.history
            .push(ChatMessage::assistant(full_response.clone()));
        Ok(full_response)
    }
}
