use crate::config::{Config, LlmConfig};
use crate::llm::{LlmBackend, Message, Role};
use std::fs;
use std::io::{self, Read, Write, stdout};
use std::path::PathBuf;

const REPHRASE_SYSTEM: &str = "\
You are a precise writing assistant. Given input text, you:
1. Fix all spelling and grammar errors
2. Flag any dubious factual claims
3. Provide rephrased versions in distinct tones

Output exactly this format:

**Corrected:**
[Original text with only spelling/grammar fixes applied]

**Concise:**
[Shortest possible version preserving full meaning]

**Professional:**
[Formal, business-appropriate tone]

**Casual:**
[Friendly, conversational tone]

**Academic:**
[Scholarly, precise tone]

Rules:
- Do NOT add information absent from the original
- If you spot a likely factual error, note it briefly under a **Fact Check:** section
- Keep each version self-contained";

pub fn run_rephrase(
    text: Option<String>,
    input_file: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let content = match (text, input_file) {
        (Some(t), _) => t,
        (None, Some(path)) => {
            eprintln!("Reading from {}...", path.display());
            fs::read_to_string(&path)?
        }
        (None, None) => {
            eprintln!("Reading from stdin (Ctrl+D to finish)...");
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;
            buf
        }
    };

    let content = content.trim().to_string();
    if content.is_empty() {
        eprintln!("No input text provided.");
        return Ok(());
    }

    let config = Config::load();
    let mut backend = create_backend(&config.llm, REPHRASE_SYSTEM)?;

    eprintln!("Rephrasing...\n");

    let messages = vec![Message {
        role: Role::User,
        content: format!(
            "Rephrase the following text. Correct spelling and grammar, check facts, and provide multiple tones.\n\n{}",
            content
        ),
    }];

    backend.generate(&messages, &mut |token| {
        print!("{}", token);
        let _ = stdout().flush();
    })?;

    println!("\n");
    Ok(())
}

fn create_backend(
    llm_config: &LlmConfig,
    _system_prompt: &str,
) -> Result<Box<dyn LlmBackend>, Box<dyn std::error::Error + Send + Sync>> {
    match llm_config {
        #[cfg(feature = "llama-cpp")]
        LlmConfig::LlamaCpp {
            model_path,
            hf_repo,
            hf_file,
            prompt_format,
            ctx_size,
        } => {
            let backend = if let Some(path) = model_path {
                crate::llm::llama::LlamaCppBackend::from_path(
                    path,
                    _system_prompt,
                    *prompt_format,
                    *ctx_size,
                )?
            } else {
                crate::llm::llama::LlamaCppBackend::from_hf(
                    hf_repo,
                    hf_file,
                    _system_prompt,
                    *prompt_format,
                    *ctx_size,
                )?
            };
            Ok(Box::new(backend))
        }
        #[cfg(not(feature = "llama-cpp"))]
        LlmConfig::LlamaCpp { .. } => {
            Err("llama-cpp not enabled. Build with --features llama-cpp".into())
        }
        #[cfg(feature = "ollama")]
        LlmConfig::Ollama { model } => Ok(Box::new(crate::llm::ollama::OllamaBackend::new(
            model,
            _system_prompt,
        ))),
        #[cfg(not(feature = "ollama"))]
        LlmConfig::Ollama { .. } => Err("Ollama not enabled. Build with --features ollama".into()),
        #[cfg(feature = "openai-compat")]
        LlmConfig::OpenAiCompat {
            base_url,
            model,
            api_key,
            temperature,
            top_p,
            max_tokens,
            presence_penalty,
            frequency_penalty,
            ..
        } => Ok(Box::new(
            crate::llm::openai_compat::OpenAiCompatBackend::new(
                base_url.clone(),
                model.clone(),
                api_key.clone(),
                *temperature,
                *top_p,
                *max_tokens,
                *presence_penalty,
                *frequency_penalty,
            )?,
        )),
        #[cfg(not(feature = "openai-compat"))]
        LlmConfig::OpenAiCompat { .. } => {
            Err("OpenAI-compatible backend not enabled. Build with --features openai-compat".into())
        }
        LlmConfig::Kalosm { .. } => Err("Kalosm backend not supported for rephrase command".into()),
    }
}
