use rodio::{OutputStreamBuilder, Sink};

pub trait TtsEngine: Send + Sync {
    fn synthesize(&self, text: &str) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>>;
}

// ============================================================================
// Kokoro TTS Engine
// ============================================================================

#[cfg(feature = "kokoro")]
pub struct KokoroEngine {
    engine: kokoros::tts::koko::TTSKoko,
    style: String, // Good choices: af_heart af_bella af_nova bf_emma am_adam am_michael am_liam
    speed: f32,
}

#[cfg(feature = "kokoro")]
impl KokoroEngine {
    pub async fn new(model_path: &str, voices_path: &str, speed: f32) -> Self {
        Self {
            engine: kokoros::tts::koko::TTSKoko::new(model_path, voices_path).await,
            style: "af_heart".to_string(),
            speed,
        }
    }
}

#[cfg(feature = "kokoro")]
impl TtsEngine for KokoroEngine {
    fn synthesize(&self, text: &str) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>> {
        let audio = self.engine.tts_raw_audio(
            text,
            "en-us",
            &self.style,
            self.speed,
            None,
            None,
            None,
            None,
        )?;
        Ok((audio, 24000))
    }
}

// ============================================================================
// Supertonic TTS Engine
// ============================================================================

#[cfg(feature = "supertonic")]
use crate::supertonic;
#[cfg(feature = "supertonic")]
use std::sync::Mutex;

#[cfg(feature = "supertonic")]
pub struct SupertonicEngine {
    tts: Mutex<supertonic::TextToSpeech>,
    style: supertonic::Style,
    total_step: usize,
    speed: f32,
}

#[cfg(feature = "supertonic")]
impl SupertonicEngine {
    pub fn new(
        onnx_dir: &str,
        voice_style_path: &str,
        speed: f32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let tts = supertonic::load_text_to_speech(onnx_dir, false)?;
        let style = supertonic::load_voice_style(&[voice_style_path.to_string()], false)?;
        Ok(Self {
            tts: Mutex::new(tts),
            style,
            total_step: 5,
            speed,
        })
    }
}

#[cfg(feature = "supertonic")]
impl TtsEngine for SupertonicEngine {
    fn synthesize(&self, text: &str) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>> {
        let mut tts = self.tts.lock().unwrap();
        let sample_rate = tts.sample_rate;
        let (wav, _) = tts.call(text, &self.style, self.total_step, self.speed, 0.3)?;
        Ok((wav, sample_rate as u32))
    }
}

// ============================================================================
// Unified TTS wrapper
// ============================================================================

pub struct Tts {
    engine: Box<dyn TtsEngine>,
}

impl Tts {
    pub fn new(engine: Box<dyn TtsEngine>) -> Self {
        Self { engine }
    }

    pub fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (audio, sample_rate) = self.engine.synthesize(text)?;
        let stream = OutputStreamBuilder::open_default_stream()?;
        let sink = Sink::connect_new(stream.mixer());
        sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, audio));
        sink.sleep_until_end();
        Ok(())
    }

    pub fn queue(&self, text: &str, sink: &Sink) -> Result<(), Box<dyn std::error::Error>> {
        let (audio, sample_rate) = self.engine.synthesize(text)?;
        sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, audio));
        Ok(())
    }

    pub fn create_sink() -> Result<(rodio::OutputStream, Sink), Box<dyn std::error::Error>> {
        let stream = OutputStreamBuilder::open_default_stream()?;
        let sink = Sink::connect_new(stream.mixer());
        Ok((stream, sink))
    }

    /// Wait for sink to finish and suppress drop warning
    pub fn finish(stream: rodio::OutputStream, sink: Sink) {
        sink.sleep_until_end();
        std::mem::forget(stream); // Suppress "Dropping OutputStream" warning
    }
}
