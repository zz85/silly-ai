use vad_rs::Vad;

const VAD_THRESHOLD: f32 = 0.3;
const VAD_THRESHOLD_END: f32 = 0.25;
const ENERGY_THRESHOLD: f32 = 0.01;
const ENERGY_THRESHOLD_END: f32 = 0.006;

pub enum VadEngine {
    Silero(Vad),
    Energy,
}

impl VadEngine {
    #[cfg(all(feature = "supertonic", target_arch = "aarch64", target_os = "macos"))]
    pub fn silero_with_gpu(model_path: &str, sample_rate: usize) -> Result<Self, String> {
        println!("Enabling CoreML acceleration for VAD...");
        let vad = Vad::new(model_path, sample_rate).map_err(|e| e.to_string())?;
        Ok(VadEngine::Silero(vad))
    }

    pub fn silero(model_path: &str, sample_rate: usize) -> Result<Self, String> {
        let vad = Vad::new(model_path, sample_rate).map_err(|e| e.to_string())?;
        Ok(VadEngine::Silero(vad))
    }

    pub fn energy() -> Self {
        VadEngine::Energy
    }

    pub fn is_speech(&mut self, frame: &[f32], currently_speaking: bool) -> bool {
        let threshold = if currently_speaking {
            match self {
                VadEngine::Silero(_) => VAD_THRESHOLD_END,
                VadEngine::Energy => ENERGY_THRESHOLD_END,
            }
        } else {
            match self {
                VadEngine::Silero(_) => VAD_THRESHOLD,
                VadEngine::Energy => ENERGY_THRESHOLD,
            }
        };

        match self {
            VadEngine::Silero(vad) => vad
                .compute(frame)
                .map(|r| r.prob > threshold)
                .unwrap_or(false),
            VadEngine::Energy => {
                let rms = (frame.iter().map(|&s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
                rms > threshold
            }
        }
    }

    pub fn reset(&mut self) {
        if let VadEngine::Silero(vad) = self {
            vad.reset();
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            VadEngine::Silero(_) => "Silero",
            VadEngine::Energy => "Energy",
        }
    }
}
