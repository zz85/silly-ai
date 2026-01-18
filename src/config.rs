use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_wake_word")]
    pub wake_word: String,
    #[serde(default = "default_wake_timeout")]
    pub wake_timeout_secs: u64,
    #[serde(default)]
    pub tts: TtsConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub acceleration: AccelerationConfig,
    #[serde(default)]
    pub interaction: InteractionConfig,
    #[serde(default)]
    pub commands: CommandsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: default_name(),
            wake_word: default_wake_word(),
            wake_timeout_secs: default_wake_timeout(),
            tts: TtsConfig::default(),
            llm: LlmConfig::default(),
            acceleration: AccelerationConfig::default(),
            interaction: InteractionConfig::default(),
            commands: CommandsConfig::default(),
        }
    }
}

// ============================================================================
// Interaction Config
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct InteractionConfig {
    /// Enable processing input while TTS is playing (can be toggled at runtime)
    #[serde(default = "default_crosstalk")]
    pub crosstalk: bool,

    /// Volume level when user speaks during TTS (0.0-1.0)
    /// Used in Phase 2 (TTS Controller) and Phase 3 (Crosstalk)
    #[serde(default = "default_duck_volume")]
    #[allow(dead_code)]
    pub duck_volume: f32,
}

impl Default for InteractionConfig {
    fn default() -> Self {
        Self {
            crosstalk: default_crosstalk(),
            duck_volume: default_duck_volume(),
        }
    }
}

fn default_crosstalk() -> bool {
    false
}

fn default_duck_volume() -> f32 {
    0.2
}

// ============================================================================
// Commands Config
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CommandsConfig {
    /// Enable built-in commands
    #[serde(default = "default_enable_builtin")]
    pub enable_builtin: bool,

    /// Phrases that stop TTS but don't go to LLM
    #[serde(default = "default_stop_phrases")]
    pub stop_phrases: Vec<String>,

    /// Custom command mappings
    #[serde(default)]
    pub custom: Vec<CustomCommand>,
}

impl Default for CommandsConfig {
    fn default() -> Self {
        Self {
            enable_builtin: default_enable_builtin(),
            stop_phrases: default_stop_phrases(),
            custom: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CustomCommand {
    pub phrase: String,
    pub action: String,
}

fn default_enable_builtin() -> bool {
    true
}

fn default_stop_phrases() -> Vec<String> {
    vec![
        "stop".to_string(),
        "quiet".to_string(),
        "shut up".to_string(),
        "enough".to_string(),
    ]
}

#[derive(Debug, Deserialize, Default)]
pub struct AccelerationConfig {
    #[serde(default = "default_tts_gpu")]
    pub tts_gpu: bool,
    #[serde(default = "default_vad_gpu")]
    pub vad_gpu: bool,
}

fn default_tts_gpu() -> bool {
    true
}

fn default_vad_gpu() -> bool {
    false
}

fn default_name() -> String {
    "Silly".into()
}
fn default_wake_word() -> String {
    "Hey Silly".into()
}
fn default_wake_timeout() -> u64 {
    30
}

// ============================================================================
// LLM Config
// ============================================================================

#[derive(Debug, Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "lowercase")]
pub enum PromptFormat {
    #[default]
    ChatML, // TinyLlama, Qwen, etc: <|im_start|>...<|im_end|>
    Mistral, // Mistral: [INST]...[/INST]
    Llama3,  // Llama 3: <|begin_of_text|>...<|eot_id|>
}

#[derive(Debug, Deserialize)]
#[serde(tag = "backend")]
#[allow(dead_code)]
pub enum LlmConfig {
    #[serde(rename = "llama-cpp")]
    LlamaCpp {
        /// Local path to GGUF model, or will download from HuggingFace
        #[serde(default)]
        model_path: Option<String>,
        /// HuggingFace repo (e.g., "TheBloke/Mistral-7B-Instruct-v0.2-GGUF")
        #[serde(default = "default_hf_repo")]
        hf_repo: String,
        /// GGUF filename in the repo
        #[serde(default = "default_hf_file")]
        hf_file: String,
        /// Prompt format
        #[serde(default)]
        prompt_format: PromptFormat,
        /// Context size (default 4096)
        #[serde(default = "default_ctx_size")]
        ctx_size: u32,
    },
    #[serde(rename = "ollama")]
    Ollama {
        #[serde(default = "default_ollama_model")]
        model: String,
    },
    #[serde(rename = "lm-studio")]
    LmStudio {
        #[serde(default = "default_lm_studio_url")]
        base_url: String,
        #[serde(default = "default_lm_studio_model")]
        model: String,
        #[serde(default = "default_ctx_size")]
        ctx_size: u32,
        temperature: Option<f32>,
        top_p: Option<f32>,
        top_k: Option<u32>,
        repetition_penalty: Option<f32>,
    },
    #[serde(rename = "kalosm")]
    Kalosm {
        /// Model preset: "phi3", "llama3-8b", "mistral-7b", "qwen-0.5b", "qwen-1.5b"
        #[serde(default = "default_kalosm_model")]
        model: String,
    },
}

impl Default for LlmConfig {
    fn default() -> Self {
        #[cfg(feature = "llama-cpp")]
        {
            LlmConfig::LlamaCpp {
                model_path: None,
                hf_repo: default_hf_repo(),
                hf_file: default_hf_file(),
                prompt_format: PromptFormat::default(),
                ctx_size: default_ctx_size(),
            }
        }
        #[cfg(all(feature = "ollama", not(feature = "llama-cpp")))]
        {
            LlmConfig::Ollama {
                model: default_ollama_model(),
            }
        }
        #[cfg(not(any(feature = "llama-cpp", feature = "ollama")))]
        {
            panic!("No LLM backend enabled. Build with --features llama-cpp or --features ollama");
        }
    }
}

fn default_hf_repo() -> String {
    "TheBloke/Mistral-7B-Instruct-v0.2-GGUF".into()
}

fn default_hf_file() -> String {
    "mistral-7b-instruct-v0.2.Q4_K_M.gguf".into()
}

fn default_ctx_size() -> u32 {
    4096
}

fn default_ollama_model() -> String {
    "mistral:7b-instruct".into()
}

fn default_lm_studio_url() -> String {
    "http://localhost:1234".into()
}

fn default_lm_studio_model() -> String {
    "default".into()
}

fn default_kalosm_model() -> String {
    "qwen-1.5b".into()
}

// ============================================================================
// TTS Config
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(tag = "engine")]
#[allow(dead_code)]
pub enum TtsConfig {
    #[serde(rename = "kokoro")]
    Kokoro {
        #[serde(default = "default_kokoro_model")]
        model: String,
        #[serde(default = "default_kokoro_voices")]
        voices: String,
        #[serde(default = "default_tts_speed")]
        speed: f32,
    },
    #[serde(rename = "supertonic")]
    Supertonic {
        #[serde(default = "default_supertonic_onnx_dir")]
        onnx_dir: String,
        #[serde(default = "default_supertonic_voice_style")]
        voice_style: String,
        #[serde(default = "default_tts_speed")]
        speed: f32,
    },
}

impl Default for TtsConfig {
    fn default() -> Self {
        #[cfg(feature = "supertonic")]
        {
            TtsConfig::Supertonic {
                onnx_dir: default_supertonic_onnx_dir(),
                voice_style: default_supertonic_voice_style(),
                speed: default_tts_speed(),
            }
        }
        #[cfg(all(feature = "kokoro", not(feature = "supertonic")))]
        {
            TtsConfig::Kokoro {
                model: default_kokoro_model(),
                voices: default_kokoro_voices(),
                speed: default_tts_speed(),
            }
        }
        #[cfg(not(any(feature = "kokoro", feature = "supertonic")))]
        {
            panic!("No TTS engine enabled. Build with --features kokoro or --features supertonic");
        }
    }
}

fn default_kokoro_model() -> String {
    "models/kokoro-v1.0.onnx".into()
}
fn default_kokoro_voices() -> String {
    "models/voices-v1.0.bin".into()
}
fn default_supertonic_onnx_dir() -> String {
    "models/supertonic/onnx".into()
}
fn default_supertonic_voice_style() -> String {
    "models/supertonic/voice_styles/M1.json".into()
}
fn default_tts_speed() -> f32 {
    1.1
}

impl Config {
    pub fn load() -> Self {
        let path = Path::new("config.toml");
        if path.exists() {
            fs::read_to_string(path)
                .ok()
                .and_then(|s| toml::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Config::default()
        }
    }
}
