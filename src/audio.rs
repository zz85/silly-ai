use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::time::{Duration, Instant};

use crate::vad::VadEngine;

const TARGET_RATE: usize = 16000;
const CHUNK_SECONDS: f32 = 3.0;
const PREVIEW_INTERVAL: Duration = Duration::from_millis(500);
const RESAMPLE_CHUNK: usize = 1024;
const MIN_PREVIEW_SAMPLES: usize = TARGET_RATE / 2;

// VAD settings - 30ms frames at 16kHz = 480 samples
const VAD_FRAME_SAMPLES: usize = 480;
const VAD_MIN_SPEECH_SAMPLES: usize = TARGET_RATE / 2;
const VAD_MAX_SPEECH_SECONDS: f32 = 10.0;
const VAD_SILENCE_FRAMES_TO_END: usize = 15;
const VAD_PREFILL_FRAMES: usize = 10;
const VAD_ONSET_FRAMES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
enum VadState {
    Idle,
    Onset(usize),
    Speaking(usize),
}

struct FrameResampler {
    resampler: Option<FftFixedIn<f32>>,
    in_buf: Vec<f32>,
    pending: Vec<f32>,
    frame_samples: usize,
}

impl FrameResampler {
    fn new(in_hz: usize, out_hz: usize, frame_samples: usize) -> Self {
        let resampler = (in_hz != out_hz)
            .then(|| FftFixedIn::<f32>::new(in_hz, out_hz, RESAMPLE_CHUNK, 1, 1).unwrap());
        Self {
            resampler,
            in_buf: Vec::with_capacity(RESAMPLE_CHUNK),
            pending: Vec::with_capacity(frame_samples),
            frame_samples,
        }
    }

    fn push(&mut self, src: &[f32], mut emit: impl FnMut(&[f32])) {
        if self.resampler.is_none() {
            self.emit_frames(src, &mut emit);
            return;
        }

        self.in_buf.extend_from_slice(src);

        while self.in_buf.len() >= RESAMPLE_CHUNK {
            let chunk: Vec<f32> = self.in_buf.drain(..RESAMPLE_CHUNK).collect();
            if let Ok(out) = self.resampler.as_mut().unwrap().process(&[&chunk], None) {
                self.emit_frames(&out[0], &mut emit);
            }
        }
    }

    fn emit_frames(&mut self, data: &[f32], emit: &mut impl FnMut(&[f32])) {
        self.pending.extend_from_slice(data);

        while self.pending.len() >= self.frame_samples {
            let frame: Vec<f32> = self.pending.drain(..self.frame_samples).collect();
            emit(&frame);
        }
    }
}

/// Start audio capture - sends raw mono samples to channel
pub fn start_capture(
    tx: Sender<Vec<f32>>,
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

    let mut resampler = FrameResampler::new(input_rate, TARGET_RATE, VAD_FRAME_SAMPLES);

    let stream = device.build_input_stream(
        &supported.config(),
        move |data: &[f32], _| {
            // Convert to mono
            let mono: Vec<f32> = if channels == 1 {
                data.to_vec()
            } else {
                data.chunks(channels)
                    .map(|c| c.iter().sum::<f32>() / channels as f32)
                    .collect()
            };

            // Resample and send frames
            resampler.push(&mono, |frame| {
                let _ = tx.send(frame.to_vec());
            });
        },
        |err| eprintln!("Stream error: {}", err),
        None,
    )?;

    stream.play()?;
    Ok(stream)
}

/// VAD processor - runs on separate thread
/// final_tx: preserves all events, preview_tx: lossy (capacity 1)
pub fn run_vad_processor(
    rx: Receiver<Vec<f32>>,
    final_tx: Sender<Vec<f32>>,
    preview_tx: SyncSender<Vec<f32>>,
    mut vad: Option<VadEngine>,
) {
    let mut state = VadState::Idle;
    let mut speech_buf: Vec<f32> =
        Vec::with_capacity((TARGET_RATE as f32 * VAD_MAX_SPEECH_SECONDS) as usize);
    let mut prefill_buf: std::collections::VecDeque<Vec<f32>> =
        std::collections::VecDeque::with_capacity(VAD_PREFILL_FRAMES + 1);
    let mut last_preview = Instant::now();
    let chunk_size = (TARGET_RATE as f32 * CHUNK_SECONDS) as usize;

    loop {
        let frame = match rx.recv() {
            Ok(f) => f,
            Err(_) => break,
        };

        if let Some(ref mut vad_engine) = vad {
            process_vad_frame(
                &frame,
                vad_engine,
                &mut state,
                &mut speech_buf,
                &mut prefill_buf,
                &mut last_preview,
                &final_tx,
                &preview_tx,
            );
        } else {
            // No VAD - fixed chunks
            speech_buf.extend_from_slice(&frame);

            let now = Instant::now();
            if speech_buf.len() >= chunk_size {
                let samples: Vec<f32> = speech_buf.drain(..chunk_size).collect();
                let _ = final_tx.send(samples);
                last_preview = now;
            } else if now.duration_since(last_preview) >= PREVIEW_INTERVAL
                && speech_buf.len() > MIN_PREVIEW_SAMPLES
            {
                // Lossy: try_send drops if full
                let _ = preview_tx.try_send(speech_buf.clone());
                last_preview = now;
            }
        }
    }
}

fn process_vad_frame(
    frame: &[f32],
    vad: &mut VadEngine,
    state: &mut VadState,
    speech_buf: &mut Vec<f32>,
    prefill_buf: &mut std::collections::VecDeque<Vec<f32>>,
    last_preview: &mut Instant,
    final_tx: &Sender<Vec<f32>>,
    preview_tx: &SyncSender<Vec<f32>>,
) {
    let is_speaking = matches!(state, VadState::Speaking(_));
    let is_speech = vad.is_speech(frame, is_speaking);

    match state {
        VadState::Idle => {
            if is_speech {
                *state = VadState::Onset(1);
            }
            prefill_buf.push_back(frame.to_vec());
            if prefill_buf.len() > VAD_PREFILL_FRAMES {
                prefill_buf.pop_front();
            }
        }
        VadState::Onset(count) => {
            if is_speech {
                *count += 1;
                prefill_buf.push_back(frame.to_vec());
                if prefill_buf.len() > VAD_PREFILL_FRAMES {
                    prefill_buf.pop_front();
                }

                if *count >= VAD_ONSET_FRAMES {
                    for pf in prefill_buf.drain(..) {
                        speech_buf.extend_from_slice(&pf);
                    }
                    *state = VadState::Speaking(0);
                }
            } else {
                *state = VadState::Idle;
                prefill_buf.push_back(frame.to_vec());
                if prefill_buf.len() > VAD_PREFILL_FRAMES {
                    prefill_buf.pop_front();
                }
            }
        }
        VadState::Speaking(silence_count) => {
            speech_buf.extend_from_slice(frame);

            if is_speech {
                *silence_count = 0;
            } else {
                *silence_count += 1;
            }
        }
    }

    // Check emit
    let should_emit = match state {
        VadState::Speaking(silence) => {
            *silence >= VAD_SILENCE_FRAMES_TO_END
                || speech_buf.len() >= (TARGET_RATE as f32 * VAD_MAX_SPEECH_SECONDS) as usize
        }
        _ => false,
    };

    if should_emit {
        if speech_buf.len() >= VAD_MIN_SPEECH_SAMPLES {
            let samples = std::mem::take(speech_buf);
            let _ = final_tx.send(samples);
        } else {
            speech_buf.clear();
        }
        *state = VadState::Idle;
        *last_preview = Instant::now();
        return;
    }

    // Preview - lossy via try_send
    if matches!(state, VadState::Speaking(_)) {
        let now = Instant::now();
        if speech_buf.len() > VAD_MIN_SPEECH_SAMPLES
            && now.duration_since(*last_preview) >= PREVIEW_INTERVAL
        {
            let _ = preview_tx.try_send(speech_buf.clone());
            *last_preview = now;
        }
    }
}
