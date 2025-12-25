use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use std::sync::mpsc::Sender;
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
const VAD_SILENCE_FRAMES_TO_END: usize = 15; // ~450ms of silence ends utterance
const VAD_PREFILL_FRAMES: usize = 10; // ~300ms of audio before speech detection
const VAD_ONSET_FRAMES: usize = 0; // ~90ms consecutive speech to trigger

pub enum AudioEvent {
    Preview(Vec<f32>),
    Final(Vec<f32>),
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

struct AudioProcessor {
    resampler: FrameResampler,
    vad: Option<VadEngine>,
    speech_buf: Vec<f32>,
    prefill_buf: std::collections::VecDeque<Vec<f32>>,
    last_preview: Instant,
    chunk_size: usize,
    channels: usize,
    silence_frames: usize,
    onset_frames: usize,
    is_speaking: bool,
}

impl AudioProcessor {
    fn new(input_rate: usize, channels: usize, vad: Option<VadEngine>) -> Self {
        let chunk_size = (TARGET_RATE as f32 * CHUNK_SECONDS) as usize;
        let max_speech = (TARGET_RATE as f32 * VAD_MAX_SPEECH_SECONDS) as usize;
        Self {
            resampler: FrameResampler::new(input_rate, TARGET_RATE, VAD_FRAME_SAMPLES),
            vad,
            speech_buf: Vec::with_capacity(max_speech),
            prefill_buf: std::collections::VecDeque::with_capacity(VAD_PREFILL_FRAMES + 1),
            last_preview: Instant::now(),
            chunk_size,
            channels,
            silence_frames: 0,
            onset_frames: 0,
            is_speaking: false,
        }
    }

    #[hotpath::measure]
    fn process(&mut self, data: &[f32], tx: &Sender<AudioEvent>) {
        // Convert to mono
        let mono: Vec<f32> = if self.channels == 1 {
            data.to_vec()
        } else {
            data.chunks(self.channels)
                .map(|c| c.iter().sum::<f32>() / self.channels as f32)
                .collect()
        };

        if self.vad.is_some() {
            self.process_with_vad(&mono, tx);
        } else {
            self.process_fixed_chunks(&mono, tx);
        }
    }

    fn process_with_vad(&mut self, mono: &[f32], tx: &Sender<AudioEvent>) {
        let vad = self.vad.as_mut().unwrap();
        let speech_buf = &mut self.speech_buf;
        let prefill_buf = &mut self.prefill_buf;
        let is_speaking = &mut self.is_speaking;
        let silence_frames = &mut self.silence_frames;
        let onset_frames = &mut self.onset_frames;

        self.resampler.push(mono, |frame| {
            let is_speech = vad.is_speech(frame, *is_speaking);

            eprint!("\ronset:{} speaking:{} silence:{} buf:{}    ", 
                *onset_frames, *is_speaking, *silence_frames, speech_buf.len());

            if is_speech {
                *onset_frames += 1;
                *silence_frames = 0;

                if *is_speaking {
                    // Already speaking - just add frame
                    speech_buf.extend_from_slice(frame);
                } else if *onset_frames >= VAD_ONSET_FRAMES {
                    // Onset threshold reached - start speaking
                    *is_speaking = true;
                    // Add prefill frames
                    for prefill_frame in prefill_buf.iter() {
                        speech_buf.extend_from_slice(prefill_frame);
                    }
                    prefill_buf.clear();
                    speech_buf.extend_from_slice(frame);
                } else {
                    // Building up onset - keep in prefill
                    prefill_buf.push_back(frame.to_vec());
                    if prefill_buf.len() > VAD_PREFILL_FRAMES {
                        prefill_buf.pop_front();
                    }
                }
            } else if *is_speaking {
                speech_buf.extend_from_slice(frame);
                *silence_frames += 1;
                *onset_frames = 0;
            } else {
                // Not speaking - maintain prefill buffer
                *onset_frames = 0;
                prefill_buf.push_back(frame.to_vec());
                if prefill_buf.len() > VAD_PREFILL_FRAMES {
                    prefill_buf.pop_front();
                }
            }
        });

        // Check if we should emit
        let max_samples = (TARGET_RATE as f32 * VAD_MAX_SPEECH_SECONDS) as usize;
        if (self.silence_frames >= VAD_SILENCE_FRAMES_TO_END && self.is_speaking)
            || self.speech_buf.len() >= max_samples
        {
            self.emit_speech(tx);
        }

        // Preview
        let now = Instant::now();
        if self.is_speaking
            && self.speech_buf.len() > VAD_MIN_SPEECH_SAMPLES
            && now.duration_since(self.last_preview) >= PREVIEW_INTERVAL
        {
            let _ = tx.send(AudioEvent::Preview(self.speech_buf.clone()));
            self.last_preview = now;
        }
    }

    fn process_fixed_chunks(&mut self, mono: &[f32], tx: &Sender<AudioEvent>) {
        self.resampler.push(mono, |frame| {
            self.speech_buf.extend_from_slice(frame);
        });

        let now = Instant::now();
        if self.speech_buf.len() >= self.chunk_size {
            let samples: Vec<f32> = self.speech_buf.drain(..self.chunk_size).collect();
            let _ = tx.send(AudioEvent::Final(samples));
            self.last_preview = now;
        } else if now.duration_since(self.last_preview) >= PREVIEW_INTERVAL
            && self.speech_buf.len() > MIN_PREVIEW_SAMPLES
        {
            let _ = tx.send(AudioEvent::Preview(self.speech_buf.clone()));
            self.last_preview = now;
        }
    }

    fn emit_speech(&mut self, tx: &Sender<AudioEvent>) {
        if self.speech_buf.len() >= VAD_MIN_SPEECH_SAMPLES {
            let samples = std::mem::take(&mut self.speech_buf);
            let _ = tx.send(AudioEvent::Final(samples));
        } else {
            self.speech_buf.clear();
        }
        self.is_speaking = false;
        self.silence_frames = 0;
        self.last_preview = Instant::now();

        // Reset VAD state after utterance
        if let Some(ref mut vad) = self.vad {
            vad.reset();
        }
    }
}

pub fn start_capture(
    tx: Sender<AudioEvent>,
    vad: Option<VadEngine>,
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

    if let Some(ref v) = vad {
        println!("VAD: {} enabled", v.name());
    } else {
        println!("VAD: disabled (fixed {}s chunks)", CHUNK_SECONDS);
    }

    let mut processor = AudioProcessor::new(input_rate, channels, vad);

    let stream = device.build_input_stream(
        &supported.config(),
        move |data: &[f32], _| processor.process(data, &tx),
        |err| eprintln!("Stream error: {}", err),
        None,
    )?;

    stream.play()?;
    Ok(stream)
}
