use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::error::Error;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use transcribe_rs::{engines::parakeet::{ParakeetEngine, ParakeetModelParams}, TranscriptionEngine};

const TARGET_RATE: u32 = 16000;
const CHUNK_SECONDS: f32 = 3.0;
const RESAMPLE_CHUNK: usize = 1024;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let model_path = PathBuf::from("models/parakeet-tdt-0.6b-v3-int8");
    let mut engine = ParakeetEngine::new();
    println!("Loading model...");
    engine.load_model_with_params(&model_path, ParakeetModelParams::int8()).map_err(|e| e.to_string())?;
    println!("Model loaded. Listening...\n");

    let host = cpal::default_host();
    let device = host.default_input_device().expect("No input device");
    let supported = device.default_input_config()?;
    let input_rate = supported.sample_rate();
    let channels = supported.channels() as usize;

    println!("Input: {}Hz, {} ch -> resampling to {}Hz mono", input_rate, channels, TARGET_RATE);

    let chunk_size = (TARGET_RATE as f32 * CHUNK_SECONDS) as usize;
    let raw_buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let resampled_buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let engine = Arc::new(Mutex::new(engine));

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let resampler = Arc::new(Mutex::new(
        SincFixedIn::<f32>::new(TARGET_RATE as f64 / input_rate as f64, 2.0, params, RESAMPLE_CHUNK, 1).unwrap()
    ));

    let config = cpal::StreamConfig {
        channels: channels as u16,
        sample_rate: input_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let raw_clone = raw_buffer.clone();
    let res_clone = resampled_buffer.clone();
    let eng_clone = engine.clone();
    let resampler_clone = resampler.clone();

    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _| {
            let mono: Vec<f32> = data.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect();
            
            let mut raw = raw_clone.lock().unwrap();
            raw.extend_from_slice(&mono);

            while raw.len() >= RESAMPLE_CHUNK {
                let chunk: Vec<f32> = raw.drain(..RESAMPLE_CHUNK).collect();
                if let Ok(mut res) = resampler_clone.try_lock() {
                    if let Ok(resampled) = res.process(&[chunk], None) {
                        let mut resampled_buf = res_clone.lock().unwrap();
                        resampled_buf.extend_from_slice(&resampled[0]);

                        if resampled_buf.len() >= chunk_size {
                            let samples: Vec<f32> = resampled_buf.drain(..chunk_size).collect();
                            drop(resampled_buf);
                            if let Ok(mut eng) = eng_clone.try_lock() {
                                if let Ok(result) = eng.transcribe_samples(samples, None) {
                                    let text = result.text.trim();
                                    if !text.is_empty() {
                                        print!("{} ", text);
                                        use std::io::Write;
                                        std::io::stdout().flush().ok();
                                    }
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
    loop { std::thread::sleep(std::time::Duration::from_secs(1)); }
}
