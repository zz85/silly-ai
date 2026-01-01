//! LLM backends - llama.cpp (default) and Ollama (optional)

use crate::config::PromptFormat;
use std::path::PathBuf;

/// Chat message for conversation history
#[derive(Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Copy)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Trait for LLM backends
pub trait LlmBackend: Send {
    /// Generate streaming response, calling on_token for each token
    fn generate(
        &mut self,
        messages: &[Message],
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
}

// ============================================================================
// llama.cpp backend
// ============================================================================

#[cfg(feature = "llama-cpp")]
pub mod llama {
    use super::*;
    use llama_cpp::{LlamaModel, LlamaParams, SessionParams, standard_sampler::StandardSampler};
    use std::io::Write;

    pub struct LlamaCppBackend {
        model: LlamaModel,
        system_prompt: String,
        prompt_format: PromptFormat,
    }

    impl LlamaCppBackend {
        /// Load model from local path
        pub fn from_path(
            path: impl Into<PathBuf>,
            system_prompt: &str,
            prompt_format: PromptFormat,
        ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let path = path.into();
            eprintln!("Loading model from {:?}...", path);

            let mut params = LlamaParams::default();
            params.n_gpu_layers = 99; // Offload all layers to GPU (Metal on macOS)

            let model = LlamaModel::load_from_file(&path, params)
                .map_err(|e| format!("Failed to load model: {:?}", e))?;
            eprintln!("Model loaded.");
            Ok(Self {
                model,
                system_prompt: system_prompt.to_string(),
                prompt_format,
            })
        }

        /// Download model from HuggingFace if needed, then load
        pub fn from_hf(
            repo: &str,
            filename: &str,
            system_prompt: &str,
            prompt_format: PromptFormat,
        ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let path = download_model(repo, filename)?;
            Self::from_path(path, system_prompt, prompt_format)
        }

        fn format_prompt(&self, messages: &[Message]) -> String {
            match self.prompt_format {
                PromptFormat::ChatML => self.format_chatml(messages),
                PromptFormat::Mistral => self.format_mistral(messages),
                PromptFormat::Llama3 => self.format_llama3(messages),
            }
        }

        fn format_chatml(&self, messages: &[Message]) -> String {
            let mut prompt = String::new();
            prompt.push_str("<|im_start|>system\n");
            prompt.push_str(&self.system_prompt);
            prompt.push_str("<|im_end|>\n");

            for msg in messages {
                let role = match msg.role {
                    Role::System => continue,
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                prompt.push_str(&format!(
                    "<|im_start|>{}\n{}<|im_end|>\n",
                    role, msg.content
                ));
            }
            prompt.push_str("<|im_start|>assistant\n");
            prompt
        }

        fn format_mistral(&self, messages: &[Message]) -> String {
            let mut prompt = String::new();
            prompt.push_str("<s>[INST] ");
            prompt.push_str(&self.system_prompt);
            prompt.push_str("\n\n");

            let mut first_user = true;
            for msg in messages {
                match msg.role {
                    Role::System => {}
                    Role::User => {
                        if first_user {
                            prompt.push_str(&msg.content);
                            prompt.push_str(" [/INST]");
                            first_user = false;
                        } else {
                            prompt.push_str(" [INST] ");
                            prompt.push_str(&msg.content);
                            prompt.push_str(" [/INST]");
                        }
                    }
                    Role::Assistant => {
                        prompt.push_str(&msg.content);
                        prompt.push_str("</s>");
                    }
                }
            }
            prompt
        }

        fn format_llama3(&self, messages: &[Message]) -> String {
            let mut prompt = String::new();
            prompt.push_str("<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n");
            prompt.push_str(&self.system_prompt);
            prompt.push_str("<|eot_id|>");

            for msg in messages {
                let role = match msg.role {
                    Role::System => continue,
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                prompt.push_str(&format!(
                    "<|start_header_id|>{}<|end_header_id|>\n\n{}<|eot_id|>",
                    role, msg.content
                ));
            }
            prompt.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
            prompt
        }

        fn stop_tokens(&self) -> &[&str] {
            match self.prompt_format {
                PromptFormat::ChatML => &["<|im_end|>", "<|im_start|>"],
                PromptFormat::Mistral => &["</s>", "[INST]"],
                PromptFormat::Llama3 => &["<|eot_id|>", "<|start_header_id|>"],
            }
        }
    }

    impl LlmBackend for LlamaCppBackend {
        fn generate(
            &mut self,
            messages: &[Message],
            on_token: &mut dyn FnMut(&str),
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let prompt = self.format_prompt(messages);
            let stop_tokens = self.stop_tokens();

            let mut session_params = SessionParams::default();
            session_params.n_ctx = 4096;
            session_params.n_batch = 2048;

            let mut session = self
                .model
                .create_session(session_params)
                .map_err(|e| format!("Failed to create session: {:?}", e))?;

            session
                .advance_context(&prompt)
                .map_err(|e| format!("Failed to advance context: {:?}", e))?;

            let mut full_response = String::new();
            let completions = session
                .start_completing_with(StandardSampler::default(), 1024)
                .map_err(|e| format!("Failed to start completion: {:?}", e))?
                .into_strings();

            for token in completions {
                if stop_tokens.iter().any(|s| token.contains(s)) {
                    break;
                }
                on_token(&token);
                full_response.push_str(&token);
                let _ = std::io::stdout().flush();
            }

            Ok(full_response)
        }
    }

    /// Download model from HuggingFace Hub
    fn download_model(
        repo: &str,
        filename: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
        use hf_hub::api::sync::Api;

        eprintln!("Checking for model {} from {}...", filename, repo);
        let api = Api::new()?;
        let repo = api.model(repo.to_string());
        let path = repo.get(filename)?;
        eprintln!("Model ready at {:?}", path);
        Ok(path)
    }
}

// ============================================================================
// Ollama backend
// ============================================================================

#[cfg(feature = "ollama")]
pub mod ollama {
    use super::*;
    use ollama_rs::Ollama;
    use ollama_rs::generation::chat::ChatMessage;
    use ollama_rs::generation::chat::request::ChatMessageRequest;
    use tokio_stream::StreamExt;

    pub struct OllamaBackend {
        client: Ollama,
        model: String,
        system_prompt: String,
    }

    impl OllamaBackend {
        pub fn new(model: &str, system_prompt: &str) -> Self {
            Self {
                client: Ollama::default(),
                model: model.to_string(),
                system_prompt: system_prompt.to_string(),
            }
        }
    }

    impl LlmBackend for OllamaBackend {
        fn generate(
            &mut self,
            messages: &[Message],
            on_token: &mut dyn FnMut(&str),
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            // Build message history
            let mut chat_messages = vec![ChatMessage::system(self.system_prompt.clone())];
            for msg in messages {
                let chat_msg = match msg.role {
                    Role::System => ChatMessage::system(msg.content.clone()),
                    Role::User => ChatMessage::user(msg.content.clone()),
                    Role::Assistant => ChatMessage::assistant(msg.content.clone()),
                };
                chat_messages.push(chat_msg);
            }

            let request = ChatMessageRequest::new(self.model.clone(), chat_messages);

            // Run async in blocking context
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            let client = &self.client;
            let result = rt.block_on(async {
                let mut stream = client.send_chat_messages_stream(request).await?;
                let mut full_response = String::new();

                while let Some(Ok(chunk)) = stream.next().await {
                    let content = &chunk.message.content;
                    on_token(content);
                    full_response.push_str(content);
                }

                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(full_response)
            })?;

            Ok(result)
        }
    }
}

// ============================================================================
// LM Studio backend
// ============================================================================

#[cfg(feature = "lm-studio")]
pub mod lm_studio {
    use super::{LlmBackend, Message, Role};
    use open_agent::{ContentBlock, Message as OaMessage, TextBlock, prelude::*};

    pub struct LmStudioBackend {
        client: Option<Client>,
        base_url: String,
        model: String,
        system_prompt: String,
    }

    impl LmStudioBackend {
        pub fn new(base_url: &str, model: &str, system_prompt: &str) -> Self {
            Self {
                client: None,
                base_url: base_url.to_string(),
                model: model.to_string(),
                system_prompt: system_prompt.to_string(),
            }
        }

        fn get_or_create_client(
            &mut self,
        ) -> std::result::Result<&mut Client, Box<dyn std::error::Error + Send + Sync>> {
            if self.client.is_none() {
                let options = AgentOptions::builder()
                    .system_prompt(&self.system_prompt)
                    .model(&self.model)
                    .base_url(&self.base_url)
                    .auto_execute_tools(false)
                    .build()?;
                self.client = Some(Client::new(options)?);
            }
            Ok(self.client.as_mut().unwrap())
        }
    }

    impl LlmBackend for LmStudioBackend {
        fn generate(
            &mut self,
            messages: &[Message],
            on_token: &mut dyn FnMut(&str),
        ) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;

            let client = self.get_or_create_client()?;

            // Sync history: clear and rebuild from messages (except last user message)
            client.clear_history();
            let history = client.history_mut();
            for msg in messages.iter().take(messages.len().saturating_sub(1)) {
                let oa_msg = match msg.role {
                    Role::System => OaMessage::system(&msg.content),
                    Role::User => OaMessage::user(&msg.content),
                    Role::Assistant => {
                        OaMessage::assistant(vec![ContentBlock::Text(TextBlock::new(&msg.content))])
                    }
                };
                history.push(oa_msg);
            }

            // Get the last user message
            let user_msg = messages
                .last()
                .filter(|m| matches!(m.role, Role::User))
                .map(|m| m.content.as_str())
                .unwrap_or("");

            let result = rt.block_on(async {
                client.send(user_msg).await?;

                let mut full_response = String::new();
                while let Some(block) = client.receive().await? {
                    match block {
                        ContentBlock::Text(text) => {
                            on_token(&text.text);
                            full_response.push_str(&text.text);
                        }
                        ContentBlock::ToolUse(tool) => {
                            eprintln!("[ToolUse] {}: {}", tool.name(), tool.input());
                        }
                        ContentBlock::ToolResult(result) => {
                            eprintln!("[ToolResult] {}", result.content());
                        }
                        ContentBlock::Image(img) => {
                            eprintln!("[Image] {}", img.url());
                        }
                    }
                }
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(full_response)
            })?;

            Ok(result)
        }
    }
}
