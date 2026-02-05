//! Acoustic Echo Cancellation using aec3 crate
//!
//! When enabled, processes mic input to remove TTS audio echo.
//! AEC runs on the VAD thread since VoipAec3 is not Send.

use aec3::voip::VoipAec3;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

const AEC_SAMPLE_RATE: usize = 16000;
const CHANNELS: usize = 1;

/// Simple linear resampler for render audio
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = (samples.len() as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;
        let s0 = samples.get(idx).copied().unwrap_or(0.0);
        let s1 = samples.get(idx + 1).copied().unwrap_or(s0);
        out.push(s0 + (s1 - s0) * frac as f32);
    }
    out
}

/// Debug WAV writer - writes samples incrementally
pub struct DebugWavWriter {
    writer: BufWriter<File>,
    num_samples: u32,
}

impl DebugWavWriter {
    pub fn new(path: &str) -> std::io::Result<Self> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        Self::write_header(&mut writer, 0)?;
        writer.flush()?;
        eprintln!("Debug WAV: writing to {}", path);
        Ok(Self { writer, num_samples: 0 })
    }

    fn write_header(w: &mut BufWriter<File>, num_samples: u32) -> std::io::Result<()> {
        let sample_rate = AEC_SAMPLE_RATE as u32;
        let byte_rate = sample_rate * 2;
        let data_size = num_samples * 2;
        let file_size = 36 + data_size;

        w.seek(SeekFrom::Start(0))?;
        w.write_all(b"RIFF")?;
        w.write_all(&file_size.to_le_bytes())?;
        w.write_all(b"WAVE")?;
        w.write_all(b"fmt ")?;
        w.write_all(&16u32.to_le_bytes())?;
        w.write_all(&1u16.to_le_bytes())?;
        w.write_all(&1u16.to_le_bytes())?;
        w.write_all(&sample_rate.to_le_bytes())?;
        w.write_all(&byte_rate.to_le_bytes())?;
        w.write_all(&2u16.to_le_bytes())?;
        w.write_all(&16u16.to_le_bytes())?;
        w.write_all(b"data")?;
        w.write_all(&data_size.to_le_bytes())?;
        Ok(())
    }

    pub fn write_samples(&mut self, samples: &[f32]) {
        for &s in samples {
            let i = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            let _ = self.writer.write_all(&i.to_le_bytes());
        }
        self.num_samples += samples.len() as u32;
    }

    pub fn flush(&mut self) {
        let _ = self.writer.flush();
        let _ = Self::write_header(&mut self.writer, self.num_samples);
        let _ = self.writer.seek(SeekFrom::End(0));
        let _ = self.writer.flush();
    }
}

impl Drop for DebugWavWriter {
    fn drop(&mut self) {
        self.flush();
        eprintln!("Debug WAV: {} samples written", self.num_samples);
    }
}

/// Render frame with sample rate info
pub struct RenderFrame {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// AEC processor that runs on a single thread
pub struct AecProcessor {
    inner: VoipAec3,
    frame_samples: usize,
    render_rx: Receiver<RenderFrame>,
    debug_mic: Option<DebugWavWriter>,
    debug_aec: Option<DebugWavWriter>,
    debug_render: Option<DebugWavWriter>,
}

impl AecProcessor {
    pub fn new(render_rx: Receiver<RenderFrame>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pipeline = VoipAec3::builder(AEC_SAMPLE_RATE, CHANNELS, CHANNELS)
            .enable_high_pass(true)
            .build()?;
        let frame_samples = pipeline.capture_frame_samples();
        Ok(Self {
            inner: pipeline,
            frame_samples,
            render_rx,
            debug_mic: None,
            debug_aec: None,
            debug_render: None,
        })
    }

    pub fn with_debug(mut self, prefix: &str) -> Self {
        self.debug_mic = DebugWavWriter::new(&format!("{}_mic.wav", prefix)).ok();
        self.debug_aec = DebugWavWriter::new(&format!("{}_aec.wav", prefix)).ok();
        self.debug_render = DebugWavWriter::new(&format!("{}_render.wav", prefix)).ok();
        self
    }

    fn drain_render(&mut self) {
        loop {
            match self.render_rx.try_recv() {
                Ok(frame) => {
                    // Resample to AEC rate
                    let samples = resample(&frame.samples, frame.sample_rate, AEC_SAMPLE_RATE as u32);
                    
                    if let Some(ref mut w) = self.debug_render {
                        w.write_samples(&samples);
                    }
                    for chunk in samples.chunks(self.frame_samples) {
                        if chunk.len() == self.frame_samples {
                            let _ = self.inner.handle_render_frame(chunk);
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    pub fn process_capture(&mut self, samples: &[f32]) -> Vec<f32> {
        if let Some(ref mut w) = self.debug_mic {
            w.write_samples(samples);
        }

        self.drain_render();

        let mut out = vec![0.0f32; samples.len()];
        for (i, chunk) in samples.chunks(self.frame_samples).enumerate() {
            if chunk.len() == self.frame_samples {
                let start = i * self.frame_samples;
                let _ = self.inner.process_capture_frame(
                    chunk,
                    false,
                    &mut out[start..start + self.frame_samples],
                );
            }
        }

        if let Some(ref mut w) = self.debug_aec {
            w.write_samples(&out);
        }

        out
    }
}

pub type AecRenderTx = Sender<RenderFrame>;
