use crate::state::SharedState;
use crate::stats::{SharedStats, StatKind, Timer};
use cpal::Sample;
use rodio::{OutputStreamBuilder, Sink, Source};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub trait TtsEngine: Send + Sync {
    fn synthesize(&self, text: &str) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>>;
}

// ============================================================================
// Audio Level Monitor - wraps a Source and monitors RMS levels in real-time
// ============================================================================

/// Wrapper around a Source that monitors audio levels as samples flow through
pub struct MonitoredSource<I>
where
    I: Source,
    I::Item: Sample,
{
    input: I,
    state: SharedState,
    buffer: Vec<f32>,
    last_update: Instant,
    update_interval: Duration,
}

impl<I> MonitoredSource<I>
where
    I: Source,
    I::Item: Sample,
{
    pub fn new(input: I, state: SharedState) -> Self {
        Self {
            input,
            state,
            buffer: Vec::new(),
            last_update: Instant::now(),
            update_interval: Duration::from_millis(50), // Update every 50ms
        }
    }

    fn update_level(&mut self) {
        if self.buffer.is_empty() {
            return;
        }

        // Calculate RMS of the buffered samples
        let rms =
            (self.buffer.iter().map(|s| s * s).sum::<f32>() / self.buffer.len() as f32).sqrt();
        self.state.set_tts_level(rms);
        self.buffer.clear();
    }
}

impl<I> Iterator for MonitoredSource<I>
where
    I: Source,
    I::Item: Sample,
{
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next()?;

        // Convert sample to f32 and add to buffer
        let float_sample = sample.to_float_sample();
        self.buffer.push(float_sample);

        // Update level if enough time has passed
        if self.last_update.elapsed() >= self.update_interval {
            self.update_level();
            self.last_update = Instant::now();
        }

        Some(sample)
    }
}

impl<I> Source for MonitoredSource<I>
where
    I: Source,
    I::Item: Sample,
{
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> u16 {
        self.input.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

// ============================================================================
// TTS Controller - wraps Sink with stop/duck operations
// ============================================================================

/// Controller for TTS playback with stop and volume control
///
/// This wraps a rodio Sink and provides:
/// - Immediate stop capability
/// - Volume ducking (reduce volume when user speaks)
/// - Integration with RuntimeState for coordinated control
pub struct TtsController {
    sink: Sink,
    state: SharedState,
    base_volume: f32,
}

impl TtsController {
    /// Create a new TTS controller with the given sink and state
    pub fn new(sink: Sink, state: SharedState) -> Self {
        Self {
            sink,
            state,
            base_volume: 1.0,
        }
    }

    /// Stop playback immediately and clear the queue
    pub fn stop(&self) {
        self.sink.stop();
        self.state.tts_playing.store(false, Ordering::SeqCst);
        self.state.set_tts_level(0.0);
    }

    /// Check if TTS is currently playing
    pub fn is_playing(&self) -> bool {
        !self.sink.empty()
    }

    /// Duck the volume (reduce to the level specified in state)
    #[allow(dead_code)]
    pub fn duck(&self) {
        let duck_volume = self.state.get_tts_volume();
        self.sink.set_volume(self.base_volume * duck_volume);
    }

    /// Restore volume to full
    #[allow(dead_code)]
    pub fn restore_volume(&self) {
        self.state.restore_tts_volume();
        self.sink.set_volume(self.base_volume);
    }

    /// Set the base volume level (0.0 - 1.0)
    #[allow(dead_code)]
    pub fn set_base_volume(&mut self, volume: f32) {
        self.base_volume = volume.clamp(0.0, 1.0);
        self.sink.set_volume(self.base_volume);
    }

    /// Get the underlying sink for queueing audio
    pub fn sink(&self) -> &Sink {
        &self.sink
    }

    /// Wait for playback to complete
    pub fn wait_until_end(&self) {
        self.sink.sleep_until_end();
    }

    /// Update volume based on current state (call periodically during playback)
    pub fn update_volume(&self) {
        let state_volume = self.state.get_tts_volume();
        self.sink.set_volume(self.base_volume * state_volume);
    }

    /// Check if cancellation was requested
    pub fn is_cancel_requested(&self) -> bool {
        self.state.is_cancel_requested()
    }
}

/// Handle for controlling TTS playback from other threads
///
/// This is a lightweight handle that can be cloned and sent to other threads
/// to control TTS playback (stop, duck, etc.)
#[allow(dead_code)]
#[derive(Clone)]
pub struct TtsHandle {
    state: SharedState,
}

#[allow(dead_code)]
impl TtsHandle {
    /// Create a new handle from shared state
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }

    /// Request TTS to stop (will be picked up by the controller)
    pub fn request_stop(&self) {
        self.state.request_cancel();
    }

    /// Duck the TTS volume
    pub fn duck(&self) {
        self.state.duck_tts();
    }

    /// Restore TTS volume
    pub fn restore_volume(&self) {
        self.state.restore_tts_volume();
    }

    /// Check if TTS is currently playing
    pub fn is_playing(&self) -> bool {
        self.state.tts_playing.load(Ordering::SeqCst)
    }

    /// Set TTS volume directly
    pub fn set_volume(&self, volume: f32) {
        self.state.set_tts_volume(volume);
    }
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
        use_gpu: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let tts = supertonic::load_text_to_speech(onnx_dir, use_gpu)?;
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
    stats: Option<SharedStats>,
}

impl Tts {
    #[allow(dead_code)]
    pub fn new(engine: Box<dyn TtsEngine>) -> Self {
        Self {
            engine,
            stats: None,
        }
    }

    pub fn with_stats(engine: Box<dyn TtsEngine>, stats: SharedStats) -> Self {
        Self {
            engine,
            stats: Some(stats),
        }
    }

    #[allow(dead_code)]
    pub fn speak(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        let (audio, sample_rate) = self.engine.synthesize(text)?;
        let stream = OutputStreamBuilder::open_default_stream()?;
        let sink = Sink::connect_new(stream.mixer());
        sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, audio));
        sink.sleep_until_end();
        Ok(())
    }

    pub fn queue(&self, text: &str, sink: &Sink) -> Result<(), Box<dyn std::error::Error>> {
        let timer = self
            .stats
            .as_ref()
            .map(|s| Timer::new(s, StatKind::Tts, text.len()));
        let (audio, sample_rate) = self.engine.synthesize(text)?;
        if let Some(t) = timer {
            t.finish(audio.len());
        }
        sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, audio));
        Ok(())
    }

    pub fn create_sink() -> Result<(rodio::OutputStream, Sink), Box<dyn std::error::Error>> {
        let stream = OutputStreamBuilder::open_default_stream()?;
        let sink = Sink::connect_new(stream.mixer());
        Ok((stream, sink))
    }

    /// Create a TTS controller with the given state
    pub fn create_controller(
        state: SharedState,
    ) -> Result<(rodio::OutputStream, TtsController), Box<dyn std::error::Error>> {
        let stream = OutputStreamBuilder::open_default_stream()?;
        let sink = Sink::connect_new(stream.mixer());
        let controller = TtsController::new(sink, state);
        Ok((stream, controller))
    }

    /// Queue text to a TTS controller
    pub fn queue_to_controller(
        &self,
        text: &str,
        controller: &TtsController,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let timer = self
            .stats
            .as_ref()
            .map(|s| Timer::new(s, StatKind::Tts, text.len()));
        let (audio, sample_rate) = self.engine.synthesize(text)?;
        if let Some(t) = timer {
            t.finish(audio.len());
        }

        // Create the audio buffer source
        let source = rodio::buffer::SamplesBuffer::new(1, sample_rate, audio);

        // Wrap it in a monitored source that tracks audio levels in real-time
        let monitored_source = MonitoredSource::new(source, controller.state.clone());

        controller.sink().append(monitored_source);
        Ok(())
    }

    /// Wait for sink to finish and suppress drop warning
    pub fn finish(stream: rodio::OutputStream, sink: Sink) {
        sink.sleep_until_end();
        std::mem::forget(stream); // Suppress "Dropping OutputStream" warning
    }

    /// Finish a controller and suppress drop warning
    pub fn finish_controller(stream: rodio::OutputStream, controller: TtsController) {
        controller.wait_until_end();
        std::mem::forget(stream); // Suppress "Dropping OutputStream" warning
    }
}
