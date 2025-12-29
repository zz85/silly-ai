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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: default_name(),
            wake_word: default_wake_word(),
            wake_timeout_secs: default_wake_timeout(),
            tts: TtsConfig::default(),
        }
    }
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

#[derive(Debug, Deserialize)]
#[serde(tag = "engine")]
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
