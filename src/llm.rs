//! LLM backends - llama.cpp (default) and Ollama (optional)

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
    fn generate(&mut self, messages: &[Message], on_token: &mut dyn FnMut(&str)) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;
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
    }

    impl LlamaCppBackend {
        /// Load model from local path
        pub fn from_path(path: impl Into<PathBuf>, system_prompt: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let path = path.into();
            println!("Loading model from {:?}...", path);
            let model = LlamaModel::load_from_file(&path, LlamaParams::default())
                .map_err(|e| format!("Failed to load model: {:?}", e))?;
            println!("Model loaded.");
            Ok(Self { model, system_prompt: system_prompt.to_string() })
        }

        /// Download model from HuggingFace if needed, then load
        pub fn from_hf(repo: &str, filename: &str, system_prompt: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let path = download_model(repo, filename)?;
            Self::from_path(path, system_prompt)
        }

        fn format_prompt(&self, messages: &[Message]) -> String {
            // Simple ChatML-style format (works with most instruction-tuned models)
            let mut prompt = String::new();
            
            // Add system prompt
            prompt.push_str("<|im_start|>system\n");
            prompt.push_str(&self.system_prompt);
            prompt.push_str("<|im_end|>\n");
            
            for msg in messages {
                let role = match msg.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                prompt.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", role, msg.content));
            }
            
            prompt.push_str("<|im_start|>assistant\n");
            prompt
        }
    }

    impl LlmBackend for LlamaCppBackend {
        fn generate(&mut self, messages: &[Message], on_token: &mut dyn FnMut(&str)) -> Result<String, Box<dyn std::error::Error + Send + Sync>>
        {
            let prompt = self.format_prompt(messages);
            
            let mut session = self.model.create_session(SessionParams::default())
                .map_err(|e| format!("Failed to create session: {:?}", e))?;
            
            session.advance_context(&prompt)
                .map_err(|e| format!("Failed to advance context: {:?}", e))?;
            
            let mut full_response = String::new();
            let completions = session.start_completing_with(StandardSampler::default(), 1024)
                .map_err(|e| format!("Failed to start completion: {:?}", e))?
                .into_strings();
            
            for token in completions {
                // Stop at end token
                if token.contains("<|im_end|>") || token.contains("<|endoftext|>") {
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
    fn download_model(repo: &str, filename: &str) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
        use hf_hub::api::sync::Api;
        
        println!("Checking for model {} from {}...", filename, repo);
        let api = Api::new()?;
        let repo = api.model(repo.to_string());
        let path = repo.get(filename)?;
        println!("Model ready at {:?}", path);
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
        fn generate(&mut self, messages: &[Message], on_token: &mut dyn FnMut(&str)) -> Result<String, Box<dyn std::error::Error + Send + Sync>>
        {
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
