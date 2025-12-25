use ollama_rs::Ollama;
use ollama_rs::generation::chat::ChatMessage;
use ollama_rs::generation::chat::request::ChatMessageRequest;

const MODEL: &str = "gpt-oss:20b";

const SYSTEM_PROMPT1: &str = r#"You are a voice assistant in a conversational, voice-first environment. Your responses will be read aloud via Text-to-Speech.

Key Instructions:
1. Keep it conversational – use natural, friendly language.
2. Short, clear answers – one or two sentences unless more detail is requested.
3. Ask clarifying questions if the transcription is ambiguous.
4. Avoid long lists – keep items brief.
5. Filter out filler words (uh, um, you know) from your understanding.
6. Be tolerant of mis-transcriptions; politely correct if needed.
7. If you cannot understand, say "Could you repeat that?"
"#;

const SYSTEM_PROMPT: &str = r#"You are an AI assistant optimized for voice interaction.
Your replies will be processed by speech‑to‑text and delivered via text‑to‑speech, so:

- Output plain text only – no Markdown, no code blocks, no URLs, no emojis, no hidden metadata.
- Use short, simple sentences that read naturally when spoken.  Keep each sentence under ~25 words and limit total length to ~200 words unless the user explicitly asks for more.
- Use punctuation to indicate natural pauses: periods, commas, question marks.
- For lists, use numbered items (1., 2., 3.) or bullet points written as “•”.
- If clarification is needed, ask directly: “Could you clarify that?” or “What do you mean by …?”  Do not make assumptions.
- If a request is repeated, summarize or restate succinctly.
- Speak in plain language; avoid jargon unless the user specifically requests it.
- Begin each response with a brief acknowledgement (“Sure,” “Got it,” “Sure thing”).
- End with an invitation for the next step (“How can I help you next?”).
- Be friendly, patient, and helpful.
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

    pub async fn send(&mut self, message: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.history.push(ChatMessage::user(message.to_string()));

        let request = ChatMessageRequest::new(MODEL.to_string(), self.history.clone());
        let response = self.ollama.send_chat_messages(request).await?;

        let msg = response.message;
        self.history.push(msg.clone());
        Ok(msg.content)
    }
}
