use std::path::Path;
use transcribe_rs::{
    TranscriptionEngine,
    engines::parakeet::{ParakeetEngine, ParakeetModelParams},
};

pub struct Transcriber {
    engine: ParakeetEngine,
}

impl Transcriber {
    pub fn new(
        model_path: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut engine = ParakeetEngine::new();
        println!("Loading model... (CoreML accelerated on Apple Silicon)");

        #[cfg(all(feature = "supertonic", target_arch = "aarch64", target_os = "macos"))]
        {
            println!("Enabling CoreML execution provider for transcription...");
        }

        engine
            .load_model_with_params(model_path.as_ref(), ParakeetModelParams::int8())
            .map_err(|e| e.to_string())?;
        println!("Model loaded.");
        Ok(Self { engine })
    }

    #[hotpath::measure]
    pub fn transcribe(
        &mut self,
        samples: &[f32],
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let result = self
            .engine
            .transcribe_samples(samples.to_vec(), None)
            .map_err(|e| e.to_string())?;
        Ok(result.text.trim().to_string())
    }
}
