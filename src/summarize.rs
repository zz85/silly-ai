use crate::config::{Config, LlmConfig};
use crate::llm::{LlmBackend, Message, Role};
use std::fs;
use std::io::{Write, stdout};
use std::path::PathBuf;

const CHUNK_OVERLAP: usize = 100;

const SUMMARIZE_SYSTEM: &str = "You are a helpful assistant that summarizes transcriptions concisely. Focus on key points, decisions, and action items.";

const CHUNK_SYSTEM: &str = "You are an expert meeting summarizer.";
const CHUNK_PROMPT: &str = "Provide a concise but comprehensive summary of the following transcript chunk. Capture all key points, decisions, action items, and mentioned individuals.\n\n";

const COMBINE_SYSTEM: &str = "You are an expert at synthesizing meeting summaries.";
const COMBINE_PROMPT: &str = "The following are consecutive summaries of a meeting. Combine them into a single, coherent, and detailed narrative summary that retains all important details, organized logically.\n\n";

/// Rough token count estimation (~0.35 tokens per char)
fn rough_token_count(s: &str) -> usize {
    (s.chars().count() as f64 * 0.35).ceil() as usize
}

/// Chunk text with overlap, breaking at sentence/word boundaries
fn chunk_text(text: &str, chunk_size_tokens: usize, overlap_tokens: usize) -> Vec<String> {
    if text.is_empty() || chunk_size_tokens == 0 {
        return vec![];
    }

    let chars_per_token = 1.0 / 0.35;
    let chunk_size_chars = (chunk_size_tokens as f64 * chars_per_token).ceil() as usize;
    let overlap_chars = (overlap_tokens as f64 * chars_per_token).ceil() as usize;

    let chars: Vec<char> = text.chars().collect();
    let total_chars = chars.len();

    if total_chars <= chunk_size_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start_char = 0;
    let step = chunk_size_chars.saturating_sub(overlap_chars).max(1);

    while start_char < total_chars {
        let end_char = (start_char + chunk_size_chars).min(total_chars);

        let start_byte: usize = chars[..start_char].iter().map(|c| c.len_utf8()).sum();
        let mut end_byte: usize = chars[..end_char].iter().map(|c| c.len_utf8()).sum();

        // Break at sentence or word boundary
        if end_char < total_chars {
            let slice = &text[start_byte..end_byte];
            if let Some(pos) = slice.rfind(". ") {
                end_byte = start_byte + pos + 2;
            } else if let Some(pos) = slice.rfind(' ') {
                end_byte = start_byte + pos + 1;
            }
        }

        chunks.push(text[start_byte..end_byte].to_string());

        if end_char >= total_chars {
            break;
        }
        start_char += step;
    }

    chunks
}

pub fn run_summarize(input: PathBuf) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let content = fs::read_to_string(&input)?;
    if content.trim().is_empty() {
        eprintln!("Input file is empty.");
        return Ok(());
    }

    let config = Config::load();
    let ctx_size = get_ctx_size(&config.llm) as usize;
    let total_tokens = rough_token_count(&content);

    println!(
        "Summarizing {} ({} estimated tokens, ctx_size {})...\n",
        input.display(),
        total_tokens,
        ctx_size
    );

    let mut backend = create_backend(&config.llm, SUMMARIZE_SYSTEM)?;

    if total_tokens < ctx_size {
        // Single-pass for short transcripts
        let messages = vec![Message {
            role: Role::User,
            content: format!("Summarize this transcription:\n\n{}", content),
        }];

        backend.generate(&messages, &mut |token| {
            print!("{}", token);
            let _ = stdout().flush();
        })?;
    } else {
        // Multi-level chunking for long transcripts
        let chunks = chunk_text(&content, ctx_size - 300, CHUNK_OVERLAP);
        println!("xxxx Processing {} chunks...\n", chunks.len());

        let mut chunk_summaries = Vec::new();

        for (i, chunk) in chunks.iter().enumerate() {
            print!("Chunk {}/{}...", i + 1, chunks.len());
            let _ = stdout().flush();

            let messages = vec![Message {
                role: Role::User,
                content: format!("{}{}", CHUNK_PROMPT, chunk),
            }];

            let mut summary = String::new();

            backend.generate(&messages, &mut |token| {
                summary.push_str(token);
            })?;

            chunk_summaries.push(summary);
        }

        // Combine and produce final summary
        println!("Generating final summary...\n");
        let combined = chunk_summaries.join("\n---\n");

        let messages = vec![Message {
            role: Role::User,
            content: format!("{}{}", COMBINE_PROMPT, combined),
        }];

        backend.generate(&messages, &mut |token| {
            print!("{}", token);
            let _ = stdout().flush();
        })?;
    }

    println!("\n");
    Ok(())
}

fn get_ctx_size(llm_config: &LlmConfig) -> u32 {
    match llm_config {
        #[cfg(feature = "llama-cpp")]
        LlmConfig::LlamaCpp { ctx_size, .. } => *ctx_size,
        #[cfg(feature = "lm-studio")]
        LlmConfig::LmStudio { ctx_size, .. } => *ctx_size,
        _ => 4096,
    }
}

fn create_backend(
    llm_config: &LlmConfig,
    system_prompt: &str,
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
                    system_prompt,
                    *prompt_format,
                    *ctx_size,
                )?
            } else {
                crate::llm::llama::LlamaCppBackend::from_hf(
                    hf_repo,
                    hf_file,
                    system_prompt,
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
            system_prompt,
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
        LlmConfig::Kalosm { .. } => {
            Err("Kalosm backend not supported for summarize command".into())
        }
    }
}
