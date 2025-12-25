use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub tts: TtsConfig,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "engine")]
pub enum TtsConfig {
    #[serde(rename = "kokoro")]
    Kokoro {
        #[serde(default = "default_kokoro_model")]
        model: String,
        #[serde(default = "default_kokoro_voices")]
        voices: String,
    },
    #[serde(rename = "supertonic")]
    Supertonic {
        #[serde(default = "default_supertonic_onnx_dir")]
        onnx_dir: String,
        #[serde(default = "default_supertonic_voice_style")]
        voice_style: String,
    },
}

impl Default for TtsConfig {
    fn default() -> Self {
        TtsConfig::Kokoro {
            model: default_kokoro_model(),
            voices: default_kokoro_voices(),
        }
    }
}

fn default_kokoro_model() -> String { "models/kokoro-v1.0.onnx".into() }
fn default_kokoro_voices() -> String { "models/voices-v1.0.bin".into() }
fn default_supertonic_onnx_dir() -> String { "models/supertonic/onnx".into() }
fn default_supertonic_voice_style() -> String { "models/supertonic/voice_styles/M1.json".into() }

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
