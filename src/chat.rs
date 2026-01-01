use crate::llm::{LlmBackend, Message, Role};

pub fn system_prompt(name: &str) -> String {
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
    backend: Box<dyn LlmBackend>,
    history: Vec<Message>,
}

impl Chat {
    pub fn new(backend: Box<dyn LlmBackend>) -> Self {
        Self {
            backend,
            history: Vec::new(),
        }
    }

    /// Estimate total words in conversation history
    pub fn context_words(&self) -> usize {
        self.history
            .iter()
            .map(|m| m.content.split_whitespace().count())
            .sum()
    }

    /// Push user message to history
    pub fn history_push_user(&mut self, message: &str) {
        self.history.push(Message {
            role: Role::User,
            content: message.to_string(),
        });
    }

    /// Push assistant message to history
    pub fn history_push_assistant(&mut self, message: &str) {
        self.history.push(Message {
            role: Role::Assistant,
            content: message.to_string(),
        });
    }

    /// Remove last message from history
    pub fn history_pop(&mut self) {
        self.history.pop();
    }

    /// Generate response with streaming callback
    pub fn generate(
        &mut self,
        mut on_token: impl FnMut(&str),
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.backend.generate(&self.history, &mut on_token)
    }
}
