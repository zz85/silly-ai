//! LLM backends - llama.cpp (default), Ollama, and Kalosm (optional)

use std::path::PathBuf;
use crate::config::PromptFormat;

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
        prompt_format: PromptFormat,
    }

    impl LlamaCppBackend {
        /// Load model from local path
        pub fn from_path(path: impl Into<PathBuf>, system_prompt: &str, prompt_format: PromptFormat) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let path = path.into();
            println!("Loading model from {:?}...", path);
            
            let mut params = LlamaParams::default();
            params.n_gpu_layers = 99; // Offload all layers to GPU (Metal on macOS)
            
            let model = LlamaModel::load_from_file(&path, params)
                .map_err(|e| format!("Failed to load model: {:?}", e))?;
            println!("Model loaded.");
            Ok(Self { model, system_prompt: system_prompt.to_string(), prompt_format })
        }

        /// Download model from HuggingFace if needed, then load
        pub fn from_hf(repo: &str, filename: &str, system_prompt: &str, prompt_format: PromptFormat) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
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
                prompt.push_str(&format!("<|im_start|>{}\n{}<|im_end|>\n", role, msg.content));
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
                prompt.push_str(&format!("<|start_header_id|>{}<|end_header_id|>\n\n{}<|eot_id|>", role, msg.content));
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
        fn generate(&mut self, messages: &[Message], on_token: &mut dyn FnMut(&str)) -> Result<String, Box<dyn std::error::Error + Send + Sync>>
        {
            let prompt = self.format_prompt(messages);
            let stop_tokens = self.stop_tokens();
            
            let mut session_params = SessionParams::default();
            session_params.n_ctx = 4096;
            session_params.n_batch = 2048;
            
            let mut session = self.model.create_session(session_params)
                .map_err(|e| format!("Failed to create session: {:?}", e))?;
            
            session.advance_context(&prompt)
                .map_err(|e| format!("Failed to advance context: {:?}", e))?;
            
            let mut full_response = String::new();
            let completions = session.start_completing_with(StandardSampler::default(), 1024)
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

// ============================================================================
// Kalosm (Floneum) backend
// ============================================================================

#[cfg(feature = "kalosm")]
pub mod kalosm_backend {
    use super::*;
    use kalosm_llama::{Llama, LlamaSource};
    use kalosm_llama::prelude::TextCompletionModelExt;
    use futures_util::StreamExt;

    pub struct KalosmBackend {
        model: Llama,
        system_prompt: String,
    }

    impl KalosmBackend {
        pub fn new_blocking(source: LlamaSource, system_prompt: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            // Spawn a new thread to avoid "runtime within runtime" panic
            let system_prompt = system_prompt.to_string();
            let handle = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                
                println!("Loading Kalosm model...");
                let model = rt.block_on(async {
                    Llama::builder()
                        .with_source(source)
                        .build()
                        .await
                })?;
                println!("Model loaded.");
                
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(Self { model, system_prompt })
            });
            
            handle.join().map_err(|_| "Thread panicked")?
        }
    }

    impl LlmBackend for KalosmBackend {
        fn generate(&mut self, messages: &[Message], on_token: &mut dyn FnMut(&str)) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            // Build prompt from messages
            let mut prompt = format!("System: {}\n\n", self.system_prompt);
            for msg in messages {
                let role = match msg.role {
                    Role::System => "System",
                    Role::User => "User",
                    Role::Assistant => "Assistant",
                };
                prompt.push_str(&format!("{}: {}\n", role, msg.content));
            }
            prompt.push_str("Assistant: ");
            
            // Create a runtime since we're on a dedicated thread without tokio
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            
            let result = rt.block_on(async {
                let mut stream = self.model.complete(&prompt);
                let mut full_response = String::new();
                while let Some(token) = stream.next().await {
                    let t = token.to_string();
                    on_token(&t);
                    full_response.push_str(&t);
                }
                full_response
            });
            
            Ok(result)
        }
    }
}
