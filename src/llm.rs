//! LLM backends - llama.cpp (default) and Ollama (optional)

#[cfg(feature = "llama-cpp")]
use crate::config::PromptFormat;
#[cfg(feature = "llama-cpp")]
use std::path::PathBuf;

/// Chat message for conversation history
#[derive(Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
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
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::{AddBos, LlamaModel, Special};
    use llama_cpp_2::sampling::LlamaSampler;
    use std::io::Write;
    use std::num::NonZeroU32;

    pub struct LlamaCppBackend {
        backend: LlamaBackend,
        model: LlamaModel,
        system_prompt: String,
        prompt_format: PromptFormat,
        ctx_size: u32,
    }

    impl LlamaCppBackend {
        /// Load model from local path
        pub fn from_path(
            path: impl Into<PathBuf>,
            system_prompt: &str,
            prompt_format: PromptFormat,
            ctx_size: u32,
        ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let path = path.into();
            eprintln!("Loading model from {:?}...", path);

            let backend =
                LlamaBackend::init().map_err(|e| format!("Failed to init backend: {:?}", e))?;

            let model_params = LlamaModelParams::default().with_n_gpu_layers(1000);
            let model_params = std::pin::pin!(model_params);

            let model = LlamaModel::load_from_file(&backend, &path, &model_params)
                .map_err(|e| format!("Failed to load model: {:?}", e))?;
            eprintln!("Model loaded.");
            Ok(Self {
                backend,
                model,
                system_prompt: system_prompt.to_string(),
                prompt_format,
                ctx_size,
            })
        }

        /// Download model from HuggingFace if needed, then load
        pub fn from_hf(
            repo: &str,
            filename: &str,
            system_prompt: &str,
            prompt_format: PromptFormat,
            ctx_size: u32,
        ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let path = download_model(repo, filename)?;
            Self::from_path(path, system_prompt, prompt_format, ctx_size)
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
    }

    impl LlmBackend for LlamaCppBackend {
        fn generate(
            &mut self,
            messages: &[Message],
            on_token: &mut dyn FnMut(&str),
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let prompt = self.format_prompt(messages);

            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(self.ctx_size))
                .with_n_batch(self.ctx_size);
            let mut ctx = self
                .model
                .new_context(&self.backend, ctx_params)
                .map_err(|e| format!("Failed to create context: {:?}", e))?;

            let tokens = self
                .model
                .str_to_token(&prompt, AddBos::Never)
                .map_err(|e| format!("Failed to tokenize: {:?}", e))?;

            let mut batch = LlamaBatch::new(self.ctx_size as usize, 1);
            let last_idx = (tokens.len() - 1) as i32;
            for (i, token) in tokens.into_iter().enumerate() {
                batch
                    .add(token, i as i32, &[0], i as i32 == last_idx)
                    .map_err(|e| format!("Failed to add token: {:?}", e))?;
            }

            ctx.decode(&mut batch)
                .map_err(|e| format!("Failed to decode: {:?}", e))?;

            let mut sampler =
                LlamaSampler::chain_simple([LlamaSampler::dist(1234), LlamaSampler::greedy()]);

            let mut full_response = String::new();
            let mut n_cur = batch.n_tokens();
            let n_len = 1024i32;
            let mut decoder = encoding_rs::UTF_8.new_decoder();

            while n_cur < n_len {
                let token = sampler.sample(&ctx, batch.n_tokens() - 1);
                sampler.accept(token);

                if self.model.is_eog_token(token) {
                    break;
                }

                if let Ok(bytes) = self.model.token_to_bytes(token, Special::Tokenize) {
                    let mut output = String::with_capacity(32);
                    let _ = decoder.decode_to_string(&bytes, &mut output, false);
                    on_token(&output);
                    full_response.push_str(&output);
                    let _ = std::io::stdout().flush();
                }

                batch.clear();
                batch
                    .add(token, n_cur, &[0], true)
                    .map_err(|e| format!("Failed to add token: {:?}", e))?;

                ctx.decode(&mut batch)
                    .map_err(|e| format!("Failed to decode: {:?}", e))?;

                n_cur += 1;
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
    use serde::{Deserialize, Serialize};
    use std::io::{BufRead, BufReader};

    #[derive(Serialize)]
    struct ChatRequest {
        model: String,
        messages: Vec<ChatMessage>,
        stream: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        temperature: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        top_p: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        top_k: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        repetition_penalty: Option<f32>,
    }

    #[derive(Serialize)]
    struct ChatMessage {
        role: String,
        content: String,
    }

    #[derive(Deserialize)]
    struct ChatChunk {
        choices: Vec<ChunkChoice>,
    }

    #[derive(Deserialize)]
    struct ChunkChoice {
        delta: Delta,
    }

    #[derive(Deserialize)]
    struct Delta {
        content: Option<String>,
    }

    pub struct LmStudioBackend {
        base_url: String,
        model: String,
        system_prompt: String,
        temperature: Option<f32>,
        top_p: Option<f32>,
        top_k: Option<u32>,
        repetition_penalty: Option<f32>,
    }

    impl LmStudioBackend {
        pub fn new(
            base_url: &str,
            model: &str,
            system_prompt: &str,
            temperature: Option<f32>,
            top_p: Option<f32>,
            top_k: Option<u32>,
            repetition_penalty: Option<f32>,
        ) -> Self {
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                model: model.to_string(),
                system_prompt: system_prompt.to_string(),
                temperature,
                top_p,
                top_k,
                repetition_penalty,
            }
        }
    }

    impl LlmBackend for LmStudioBackend {
        fn generate(
            &mut self,
            messages: &[Message],
            on_token: &mut dyn FnMut(&str),
        ) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let mut chat_messages = vec![ChatMessage {
                role: "system".to_string(),
                content: self.system_prompt.clone(),
            }];

            for msg in messages {
                let role = match msg.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                chat_messages.push(ChatMessage {
                    role: role.to_string(),
                    content: msg.content.clone(),
                });
            }

            let request = ChatRequest {
                model: self.model.clone(),
                messages: chat_messages,
                stream: true,
                temperature: self.temperature,
                top_p: self.top_p,
                top_k: self.top_k,
                repetition_penalty: self.repetition_penalty,
            };

            let response =
                ureq::post(&format!("{}/chat/completions", self.base_url)).send_json(&request)?;

            let reader = BufReader::new(response.into_body().into_reader());
            let mut full_response = String::new();

            for line in reader.lines() {
                let line = line?;
                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Ok(chunk) = serde_json::from_str::<ChatChunk>(json_str) {
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(content) = &choice.delta.content {
                                on_token(content);
                                full_response.push_str(content);
                            }
                        }
                    }
                }
            }

            Ok(full_response)
        }
    }
}

// ============================================================================
// Kalosm (Floneum) backend
// ============================================================================

#[cfg(feature = "kalosm")]
pub mod kalosm_backend {
    use super::*;
    use futures_util::StreamExt;
    use kalosm_llama::prelude::TextCompletionModelExt;
    use kalosm_llama::{Llama, LlamaSource};

    pub struct KalosmBackend {
        model: Llama,
        system_prompt: String,
    }

    impl KalosmBackend {
        pub fn new_blocking(
            source: LlamaSource,
            system_prompt: &str,
        ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
            let system_prompt = system_prompt.to_string();
            let handle = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;

                println!("Loading Kalosm model...");
                let model =
                    rt.block_on(async { Llama::builder().with_source(source).build().await })?;
                println!("Model loaded.");

                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(Self {
                    model,
                    system_prompt,
                })
            });

            handle.join().map_err(|_| "Thread panicked")?
        }
    }

    impl LlmBackend for KalosmBackend {
        fn generate(
            &mut self,
            messages: &[Message],
            on_token: &mut dyn FnMut(&str),
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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
