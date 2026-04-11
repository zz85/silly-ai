use crate::stats::{SharedStats, StatKind, Timer};
use std::path::Path;
pub use transcribe_rs::TranscriptionSegment;
use transcribe_rs::{
    onnx::parakeet::ParakeetModel, onnx::Quantization, SpeechModel, TranscribeOptions,
};

pub struct Transcriber {
    engine: ParakeetModel,
    stats: Option<SharedStats>,
}

impl Transcriber {
    pub fn new(
        model_path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::with_stats(model_path, None)
    }

    pub fn with_stats(
        model_path: impl AsRef<Path>,
        stats: Option<SharedStats>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        println!("Loading model... (CoreML accelerated on Apple Silicon)");

        #[cfg(all(feature = "supertonic", target_arch = "aarch64", target_os = "macos"))]
        {
            println!("Enabling CoreML execution provider for transcription...");
        }

        let engine = ParakeetModel::load(model_path.as_ref(), &Quantization::Int8)
            .map_err(|e| e.to_string())?;
        println!("Model loaded.");
        Ok(Self { engine, stats })
    }

    #[hotpath::measure]
    pub fn transcribe(
        &mut self,
        samples: &[f32],
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Check for empty samples to prevent transcription errors
        if samples.is_empty() {
            return Ok(String::new());
        }

        let timer = self
            .stats
            .as_ref()
            .map(|s| Timer::new(s, StatKind::Transcription, samples.len()));
        let result = self
            .engine
            .transcribe(samples, &TranscribeOptions::default())
            .map_err(|e| e.to_string())?;
        let text = result.text.trim().to_string();
        if let Some(t) = timer {
            t.finish(text.len());
        }
        Ok(text)
    }

    #[allow(dead_code)]
    pub fn transcribe_with_segments(
        &mut self,
        samples: &[f32],
    ) -> Result<(String, Option<Vec<TranscriptionSegment>>), Box<dyn std::error::Error + Send + Sync>>
    {
        let result = self
            .engine
            .transcribe(samples, &TranscribeOptions::default())
            .map_err(|e| e.to_string())?;
        Ok((result.text.trim().to_string(), result.segments))
    }
}
