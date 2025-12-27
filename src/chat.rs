use ollama_rs::Ollama;
use ollama_rs::generation::chat::ChatMessage;
use ollama_rs::generation::chat::request::ChatMessageRequest;
use tokio_stream::StreamExt;
use std::pin::Pin;

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
    pub async fn greet_with_callback<F, C>(
        &mut self,
        on_sentence: F,
        on_chunk: C,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(&str),
        C: FnMut(&str),
    {
        let prompt = format!("Hello.");
        self.send_streaming_with_callback(&prompt, on_sentence, on_chunk)
            .await
    }

    /// Stream response, calling `on_sentence` for each complete sentence
    /// `on_chunk` is called for each streamed token
    pub async fn send_streaming_with_callback<F, C>(
        &mut self,
        message: &str,
        mut on_sentence: F,
        mut on_chunk: C,
    ) -> Result<String, Box<dyn std::error::Error>>
    where
        F: FnMut(&str),
        C: FnMut(&str),
    {
        self.history.push(ChatMessage::user(message.to_string()));

        let request = ChatMessageRequest::new(MODEL.to_string(), self.history.clone());

        let mut stream = self.ollama.send_chat_messages_stream(request).await?;

        let mut full_response = String::new();
        let mut buffer = String::new();

        while let Some(Ok(chunk)) = stream.next().await {
            let content = &chunk.message.content;
            on_chunk(content);
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

        self.history
            .push(ChatMessage::assistant(full_response.clone()));
        Ok(full_response)
    }

    /// Start streaming response, returns an async stream of chunks
    pub async fn send_streaming(&mut self, message: &str) -> LlmStream {
        self.history.push(ChatMessage::user(message.to_string()));
        let request = ChatMessageRequest::new(MODEL.to_string(), self.history.clone());
        
        match self.ollama.send_chat_messages_stream(request).await {
            Ok(stream) => LlmStream {
                stream: Some(Box::pin(stream)),
                buffer: String::new(),
                response: String::new(),
            },
            Err(e) => {
                eprintln!("LLM error: {}", e);
                LlmStream {
                    stream: None,
                    buffer: String::new(),
                    response: String::new(),
                }
            }
        }
    }

    /// Finish response and add to history
    pub fn finish_response(&mut self, response: String) {
        if !response.is_empty() {
            self.history.push(ChatMessage::assistant(response));
        }
    }
}

pub struct LlmChunk {
    pub text: Option<String>,
    pub sentence: Option<String>,
}

pub struct LlmStream {
    stream: Option<Pin<Box<dyn tokio_stream::Stream<Item = Result<ollama_rs::generation::chat::ChatMessageResponse, ()>> + Send>>>,
    buffer: String,
    pub response: String,
}

impl LlmStream {
    pub async fn next(&mut self) -> Option<LlmChunk> {
        let stream = self.stream.as_mut()?;
        
        if let Some(Ok(chunk)) = stream.next().await {
            let content = chunk.message.content;
            self.response.push_str(&content);
            self.buffer.push_str(&content);
            
            // Check for complete sentence
            let sentence = if let Some(pos) = self.buffer.find(|c| c == '.' || c == '!' || c == '?') {
                let s = self.buffer[..=pos].trim().to_string();
                self.buffer = self.buffer[pos + 1..].to_string();
                if s.is_empty() { None } else { Some(s) }
            } else {
                None
            };
            
            Some(LlmChunk {
                text: Some(content),
                sentence,
            })
        } else {
            // Stream ended, flush remaining buffer
            if !self.buffer.is_empty() {
                let remaining = std::mem::take(&mut self.buffer);
                let trimmed = remaining.trim();
                if !trimmed.is_empty() {
                    return Some(LlmChunk {
                        text: None,
                        sentence: Some(trimmed.to_string()),
                    });
                }
            }
            None
        }
    }
}
