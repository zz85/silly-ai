mod audio;
mod transcriber;
mod vad;

use std::error::Error;
use std::io::Write;
use std::sync::mpsc;

use vad::VadEngine;

const VAD_MODEL_PATH: &str = "models/silero_vad_v4.onnx";
const TARGET_RATE: usize = 16000;

#[hotpath::main]
fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (tx, rx) = hotpath::channel!(mpsc::channel());

    let mut transcriber = transcriber::Transcriber::new("models/parakeet-tdt-0.6b-v3-int8")?;

    // Try Silero VAD, fall back to energy-based
    let vad = if std::path::Path::new(VAD_MODEL_PATH).exists() {
        match VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("Silero VAD failed ({}), using energy-based", e);
                Some(VadEngine::energy())
            }
        }
    } else {
        eprintln!("VAD model not found, using energy-based");
        Some(VadEngine::energy())
    };

    let _stream = audio::start_capture(tx, vad)?;

    println!("Listening... Press Ctrl+C to stop.\n");

    let mut preview_text = String::new();

    loop {
        let event = match rx.recv() {
            Ok(e) => e,
            Err(_) => break,
        };

        match event {
            audio::AudioEvent::Preview(samples) => {
                // Skip preview if samples are small
                if samples.len() < 8000 {
                    continue;
                }
                if let Ok(text) = transcriber.transcribe(&samples) {
                    if !text.is_empty() && text != preview_text {
                        preview_text = text.clone();
                        print!("\r\x1b[K\x1b[90m{}\x1b[0m", text);
                        std::io::stdout().flush().ok();
                    }
                }
            }
            audio::AudioEvent::Final(samples) => {
                // Drain any queued Final events to prevent backlog
                let mut latest_samples = samples;
                while let Ok(queued) = rx.try_recv() {
                    if let audio::AudioEvent::Final(s) = queued {
                        latest_samples = s;
                    }
                }

                if let Ok(text) = transcriber.transcribe(&latest_samples) {
                    if !text.is_empty() {
                        print!("\r\x1b[K{}\n", text);
                        std::io::stdout().flush().ok();
                    }
                }
                preview_text.clear();
            }
        }
    }

    Ok(())
}
