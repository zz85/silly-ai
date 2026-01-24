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
    #[serde(default)]
    pub ui: UiConfig,
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
            ui: UiConfig::default(),
        }
    }
}

// ============================================================================
// UI Config
// ============================================================================

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiModeConfig {
    /// Text-based terminal UI (default)
    #[default]
    Text,
    /// Orb visualization mode
    Orb,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OrbStyleConfig {
    /// Simple rotating ring
    Ring,
    /// Volumetric noise blob
    #[default]
    Blob,
    /// Concentric glowing orbs
    Orbs,
}

#[derive(Debug, Deserialize)]
pub struct UiConfig {
    /// UI mode: "text" or "graphical"
    #[serde(default)]
    pub mode: UiModeConfig,
    /// Visual style for graphical mode: "ring", "blob", or "orbs"
    #[serde(default)]
    pub orb_style: OrbStyleConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            mode: UiModeConfig::default(),
            orb_style: OrbStyleConfig::default(),
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
    #[serde(default = "default_duck_volume")]
    #[allow(dead_code)]
    pub duck_volume: f32,

    /// Enable acoustic echo cancellation (removes TTS audio from mic input)
    #[serde(default)]
    pub aec: bool,
}

impl Default for InteractionConfig {
    fn default() -> Self {
        Self {
            crosstalk: default_crosstalk(),
            duck_volume: default_duck_volume(),
            aec: false,
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
    #[serde(rename = "kalosm")]
    Kalosm {
        /// Model preset: "phi3", "llama3-8b", "mistral-7b", "qwen-0.5b", "qwen-1.5b"
        #[serde(default = "default_kalosm_model")]
        model: String,
    },
    #[serde(rename = "openai-compat")]
    OpenAiCompat {
        /// Base URL - can use preset or explicit URL
        #[serde(default)]
        base_url: String,
        /// Preset shortcuts: "lm_studio", "openai", "ollama"
        preset: Option<String>,
        /// Model name
        model: String,
        /// API key (supports ${ENV_VAR} syntax)
        #[serde(default)]
        api_key: Option<String>,
        // Sampling parameters
        temperature: Option<f32>,
        top_p: Option<f32>,
        max_tokens: Option<u32>,
        presence_penalty: Option<f32>,
        frequency_penalty: Option<f32>,
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

fn default_kalosm_model() -> String {
    "qwen-1.5b".into()
}

/// Expand ${VAR} to environment variable values
fn expand_env_vars(s: &str) -> String {
    let mut result = s.to_string();

    // Handle ${VAR} syntax
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let value = std::env::var(var_name).unwrap_or_else(|_| {
                eprintln!("Warning: Environment variable '{}' not found", var_name);
                String::new()
            });
            result.replace_range(start..start + end + 1, &value);
        } else {
            break;
        }
    }

    result
}

impl LlmConfig {
    /// Resolve preset to base_url if needed, and expand env vars in api_key
    pub fn resolve_presets(&mut self) {
        if let LlmConfig::OpenAiCompat {
            base_url,
            preset,
            api_key,
            ..
        } = self
        {
            // Resolve preset to base_url
            if base_url.is_empty() {
                if let Some(preset_name) = preset {
                    *base_url = match preset_name.as_str() {
                        "lm_studio" => "http://localhost:1234/v1".to_string(),
                        "openai" => "https://api.openai.com/v1".to_string(),
                        "ollama" => "http://localhost:11434/v1".to_string(),
                        _ => {
                            eprintln!(
                                "Warning: Unknown preset '{}', using LM Studio default",
                                preset_name
                            );
                            "http://localhost:1234/v1".to_string()
                        }
                    };
                } else {
                    // No preset and no base_url - default to LM Studio
                    *base_url = "http://localhost:1234/v1".to_string();
                }
            }

            // Expand environment variables in api_key
            if let Some(key) = api_key {
                *key = expand_env_vars(key);
            }
        }
    }
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
        let mut config = if path.exists() {
            fs::read_to_string(path)
                .ok()
                .and_then(|s| toml::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Config::default()
        };

        // Resolve presets
        config.llm.resolve_presets();

        config
    }
}
