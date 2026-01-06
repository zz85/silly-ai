use crate::vad::VadEngine;
use flume::{Receiver, Sender};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const VAD_FRAME_SAMPLES: usize = 480;
const TARGET_RATE: usize = 16000;

#[derive(Clone, Debug)]
pub struct AudioSegment {
    pub samples: Vec<f32>,
    pub start_sample: usize,
    pub end_sample: usize,
}

impl AudioSegment {
    pub fn start_secs(&self) -> f32 {
        self.start_sample as f32 / TARGET_RATE as f32
    }

    pub fn end_secs(&self) -> f32 {
        self.end_sample as f32 / TARGET_RATE as f32
    }

    pub fn duration_secs(&self) -> f32 {
        self.samples.len() as f32 / TARGET_RATE as f32
    }
}

pub struct SegmenterConfig {
    pub silence_ms: u32,
    pub max_segment_secs: u32,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self {
            silence_ms: 500,
            max_segment_secs: 30,
        }
    }
}

pub fn run_segmenter(
    rx: Receiver<Vec<f32>>,
    tx: Sender<AudioSegment>,
    mut vad: VadEngine,
    config: SegmenterConfig,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let silence_threshold_frames =
        (config.silence_ms as usize * TARGET_RATE) / (1000 * VAD_FRAME_SAMPLES);
    let max_samples = config.max_segment_secs as usize * TARGET_RATE;

    let mut vad_buf: Vec<f32> = Vec::new();
    let mut speech_buf: Vec<f32> = Vec::new();
    let mut in_speech = false;
    let mut silence_frames: u32 = 0;
    let mut total_samples: usize = 0;
    let mut speech_start_sample: usize = 0;
    let mut first_audio = true;

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(samples) => {
                if first_audio {
                    println!("First audio chunk: {} samples", samples.len());
                    first_audio = false;
                }
                vad_buf.extend_from_slice(&samples);
            }
            Err(flume::RecvTimeoutError::Timeout) => continue,
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }

        while vad_buf.len() >= VAD_FRAME_SAMPLES {
            let chunk: Vec<f32> = vad_buf.drain(..VAD_FRAME_SAMPLES).collect();
            let is_speech = vad.is_speech(&chunk, in_speech);

            if is_speech {
                if !in_speech {
                    speech_start_sample = total_samples;
                    print!("🎤 ");
                    std::io::stdout().flush().ok();
                }
                silence_frames = 0;
                in_speech = true;
                speech_buf.extend_from_slice(&chunk);
            } else if in_speech {
                silence_frames += 1;
                speech_buf.extend_from_slice(&chunk);

                if silence_frames >= silence_threshold_frames as u32
                    || speech_buf.len() >= max_samples
                {
                    let duration = speech_buf.len() as f32 / TARGET_RATE as f32;
                    println!("[{:.1}s]", duration);
                    
                    let segment = AudioSegment {
                        samples: std::mem::take(&mut speech_buf),
                        start_sample: speech_start_sample,
                        end_sample: total_samples + VAD_FRAME_SAMPLES,
                    };
                    let _ = tx.send(segment);

                    in_speech = false;
                    silence_frames = 0;
                    vad.reset();
                }
            }

            total_samples += VAD_FRAME_SAMPLES;
        }
    }

    // Flush remaining
    if !speech_buf.is_empty() && speech_buf.len() >= TARGET_RATE / 2 {
        let segment = AudioSegment {
            samples: speech_buf,
            start_sample: speech_start_sample,
            end_sample: total_samples,
        };
        let _ = tx.send(segment);
    }

    Ok(())
}
