use kokoros::tts::koko::TTSKoko;
use rodio::{OutputStreamBuilder, Sink};

const SAMPLE_RATE: u32 = 24000;

pub struct Tts {
    engine: TTSKoko,
    style: String,
    speed: f32,
}

impl Tts {
    pub async fn new(model_path: &str, voices_path: &str) -> Self {
        Self {
            engine: TTSKoko::new(model_path, voices_path).await,
            style: "af_heart".to_string(), // Good choices: af_heart af_bella af_nova bf_emma am_adam am_michael am_liam
            speed: 1.0,
        }
    }

    pub fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
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

        // Play audio
        let stream = OutputStreamBuilder::open_default_stream()?;
        let sink = Sink::connect_new(stream.mixer());

        let source = rodio::buffer::SamplesBuffer::new(1, SAMPLE_RATE, audio);
        sink.append(source);
        sink.sleep_until_end();

        Ok(())
    }
}
