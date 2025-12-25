use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

const TARGET_RATE: usize = 16000;
const CHUNK_SECONDS: f32 = 3.0;
const PREVIEW_INTERVAL: Duration = Duration::from_millis(500);
const RESAMPLE_CHUNK: usize = 1024;
const MIN_PREVIEW_SAMPLES: usize = TARGET_RATE / 2;

pub enum AudioEvent {
    Preview(Vec<f32>),
    Final(Vec<f32>),
}

struct AudioProcessor {
    resampler: Option<FftFixedIn<f32>>,
    raw_buf: Vec<f32>,
    audio_buf: Vec<f32>,
    last_preview: Instant,
    chunk_size: usize,
    channels: usize,
}

impl AudioProcessor {
    fn new(input_rate: usize, channels: usize) -> Self {
        let resampler = (input_rate != TARGET_RATE)
            .then(|| FftFixedIn::new(input_rate, TARGET_RATE, RESAMPLE_CHUNK, 1, 1).unwrap());
        Self {
            resampler,
            raw_buf: Vec::with_capacity(RESAMPLE_CHUNK * 2),
            audio_buf: Vec::with_capacity((TARGET_RATE as f32 * CHUNK_SECONDS) as usize * 2),
            last_preview: Instant::now(),
            chunk_size: (TARGET_RATE as f32 * CHUNK_SECONDS) as usize,
            channels,
        }
    }

    #[hotpath::measure]
    fn process(&mut self, data: &[f32], tx: &Sender<AudioEvent>) {
        self.convert_to_mono(data);
        self.resample();
        self.emit_events(tx);
    }

    fn convert_to_mono(&mut self, data: &[f32]) {
        if self.channels == 1 {
            self.raw_buf.extend_from_slice(data);
        } else {
            self.raw_buf.extend(
                data.chunks(self.channels)
                    .map(|c| c.iter().sum::<f32>() / self.channels as f32),
            );
        }
    }

    fn resample(&mut self) {
        while self.raw_buf.len() >= RESAMPLE_CHUNK {
            let chunk: Vec<f32> = self.raw_buf.drain(..RESAMPLE_CHUNK).collect();
            let resampled = match &mut self.resampler {
                Some(r) => r.process(&[chunk], None).ok().and_then(|mut r| r.pop()),
                None => Some(chunk),
            };
            if let Some(samples) = resampled {
                self.audio_buf.extend_from_slice(&samples);
            }
        }
    }

    fn emit_events(&mut self, tx: &Sender<AudioEvent>) {
        let now = Instant::now();
        if self.audio_buf.len() >= self.chunk_size {
            let samples: Vec<f32> = self.audio_buf.drain(..self.chunk_size).collect();
            let _ = tx.send(AudioEvent::Final(samples));
            self.last_preview = now;
        } else if now.duration_since(self.last_preview) >= PREVIEW_INTERVAL
            && self.audio_buf.len() > MIN_PREVIEW_SAMPLES
        {
            let _ = tx.send(AudioEvent::Preview(self.audio_buf.clone()));
            self.last_preview = now;
        }
    }
}

pub fn start_capture(
    tx: Sender<AudioEvent>,
) -> Result<Stream, Box<dyn std::error::Error + Send + Sync>> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No input device")?;
    let supported = device.default_input_config()?;
    let input_rate = supported.sample_rate() as usize;
    let channels = supported.channels() as usize;

    println!(
        "Audio: {}Hz {}ch -> {}Hz mono",
        input_rate, channels, TARGET_RATE
    );

    let mut processor = AudioProcessor::new(input_rate, channels);

    let stream = device.build_input_stream(
        &supported.config(),
        move |data: &[f32], _| processor.process(data, &tx),
        |err| eprintln!("Stream error: {}", err),
        None,
    )?;

    stream.play()?;
    Ok(stream)
}
