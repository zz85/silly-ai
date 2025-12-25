use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use transcribe_rs::{
    TranscriptionEngine,
    engines::parakeet::{ParakeetEngine, ParakeetModelParams},
};

const TARGET_RATE: usize = 16000;
const CHUNK_SECONDS: f32 = 3.0;
const PREVIEW_INTERVAL_MS: u64 = 500;
const RESAMPLE_CHUNK: usize = 1024;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let model_path = PathBuf::from("models/parakeet-tdt-0.6b-v3-int8");
    let mut engine = ParakeetEngine::new();
    println!("Loading model...");
    engine
        .load_model_with_params(&model_path, ParakeetModelParams::int8())
        .map_err(|e| e.to_string())?;
    println!("Model loaded. Listening...\n");

    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device");
    let supported = device.default_input_config()?;
    let input_rate = supported.sample_rate() as usize;
    let channels = supported.channels() as usize;

    println!(
        "Input: {}Hz, {} ch -> {}Hz mono",
        input_rate, channels, TARGET_RATE
    );

    let chunk_size = (TARGET_RATE as f32 * CHUNK_SECONDS) as usize;
    let raw_buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let audio_buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let engine = Arc::new(Mutex::new(engine));
    let last_preview: Arc<Mutex<Instant>> = Arc::new(Mutex::new(Instant::now()));
    let last_preview_text: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    let resampler: Arc<Mutex<Option<FftFixedIn<f32>>>> = if input_rate != TARGET_RATE {
        Arc::new(Mutex::new(Some(FftFixedIn::new(
            input_rate,
            TARGET_RATE,
            RESAMPLE_CHUNK,
            1,
            1,
        )?)))
    } else {
        Arc::new(Mutex::new(None))
    };

    let config = supported.config();

    let raw_clone = raw_buffer.clone();
    let audio_clone = audio_buffer.clone();
    let eng_clone = engine.clone();
    let resampler_clone = resampler.clone();
    let preview_clone = last_preview.clone();
    let preview_text_clone = last_preview_text.clone();

    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _| {
            let mono: Vec<f32> = if channels == 1 {
                data.to_vec()
            } else {
                data.chunks(channels)
                    .map(|c| c.iter().sum::<f32>() / channels as f32)
                    .collect()
            };

            let mut raw = raw_clone.lock().unwrap();
            raw.extend_from_slice(&mono);

            while raw.len() >= RESAMPLE_CHUNK {
                let chunk: Vec<f32> = raw.drain(..RESAMPLE_CHUNK).collect();

                let resampled = if let Ok(mut res_guard) = resampler_clone.try_lock() {
                    if let Some(ref mut res) = *res_guard {
                        res.process(&[chunk], None)
                            .ok()
                            .map(|r| r.into_iter().next().unwrap())
                    } else {
                        Some(chunk)
                    }
                } else {
                    continue;
                };

                if let Some(samples) = resampled {
                    let mut audio = audio_clone.lock().unwrap();
                    audio.extend_from_slice(&samples);
                    let audio_len = audio.len();

                    let now = Instant::now();
                    let mut last = preview_clone.lock().unwrap();

                    // Preview transcription
                    if now.duration_since(*last) >= Duration::from_millis(PREVIEW_INTERVAL_MS)
                        && audio_len > TARGET_RATE / 2
                    {
                        *last = now;
                        let preview_audio = audio.clone();
                        drop(audio);
                        drop(last);

                        if let Ok(mut eng) = eng_clone.try_lock() {
                            if let Ok(result) = eng.transcribe_samples(preview_audio, None) {
                                let text = result.text.trim();
                                let mut prev_text = preview_text_clone.lock().unwrap();
                                if !text.is_empty() && text != prev_text.as_str() {
                                    *prev_text = text.to_string();
                                    print!("\r\x1b[K\x1b[90m{}\x1b[0m", text);
                                    std::io::Write::flush(&mut std::io::stdout()).ok();
                                }
                            }
                        }
                    } else if audio_len >= chunk_size {
                        // Final transcription
                        let final_audio: Vec<f32> = audio.drain(..chunk_size).collect();
                        drop(audio);

                        if let Ok(mut eng) = eng_clone.try_lock() {
                            if let Ok(result) = eng.transcribe_samples(final_audio, None) {
                                let text = result.text.trim();
                                if !text.is_empty() {
                                    print!("\r\x1b[K{}\n", text);
                                    std::io::Write::flush(&mut std::io::stdout()).ok();
                                    *preview_text_clone.lock().unwrap() = String::new();
                                }
                            }
                        }
                    }
                }
            }
        },
        |err| eprintln!("Stream error: {}", err),
        None,
    )?;

    stream.play()?;
    println!("Press Ctrl+C to stop.\n");
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
