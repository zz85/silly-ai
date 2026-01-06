use crate::config::{Config, LlmConfig};
use crate::llm::{LlmBackend, Message, Role};
use std::fs;
use std::io::{Write, stdout};
use std::path::PathBuf;

const SUMMARIZE_SYSTEM: &str = "You are a helpful assistant that summarizes transcriptions concisely. Focus on key points, decisions, and action items.";

pub fn run_summarize(input: PathBuf) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let content = fs::read_to_string(&input)?;
    if content.trim().is_empty() {
        println!("Input file is empty.");
        return Ok(());
    }

    let config = Config::load();
    let mut backend = create_backend(&config.llm)?;

    let messages = vec![Message {
        role: Role::User,
        content: format!("Summarize this transcription:\n\n{}", content),
    }];

    println!("Summarizing {}...\n", input.display());

    backend.generate(&messages, &mut |token| {
        print!("{}", token);
        let _ = stdout().flush();
    })?;

    println!("\n");
    Ok(())
}

fn create_backend(
    llm_config: &LlmConfig,
) -> Result<Box<dyn LlmBackend>, Box<dyn std::error::Error + Send + Sync>> {
    match llm_config {
        #[cfg(feature = "llama-cpp")]
        LlmConfig::LlamaCpp {
            model_path,
            hf_repo,
            hf_file,
            prompt_format,
        } => {
            let backend = if let Some(path) = model_path {
                crate::llm::llama::LlamaCppBackend::from_path(path, SUMMARIZE_SYSTEM, *prompt_format)?
            } else {
                crate::llm::llama::LlamaCppBackend::from_hf(hf_repo, hf_file, SUMMARIZE_SYSTEM, *prompt_format)?
            };
            Ok(Box::new(backend))
        }
        #[cfg(not(feature = "llama-cpp"))]
        LlmConfig::LlamaCpp { .. } => {
            Err("llama-cpp not enabled. Build with --features llama-cpp".into())
        }
        #[cfg(feature = "ollama")]
        LlmConfig::Ollama { model } => {
            Ok(Box::new(crate::llm::ollama::OllamaBackend::new(model, SUMMARIZE_SYSTEM)))
        }
        #[cfg(not(feature = "ollama"))]
        LlmConfig::Ollama { .. } => {
            Err("Ollama not enabled. Build with --features ollama".into())
        }
        #[cfg(feature = "lm-studio")]
        LlmConfig::LmStudio { base_url, model } => {
            Ok(Box::new(crate::llm::lm_studio::LmStudioBackend::new(base_url, model, SUMMARIZE_SYSTEM)))
        }
        #[cfg(not(feature = "lm-studio"))]
        LlmConfig::LmStudio { .. } => {
            Err("LM Studio not enabled. Build with --features lm-studio".into())
        }
    }
}
