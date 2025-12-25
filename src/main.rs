mod audio;
mod transcriber;
mod vad;

use std::error::Error;
use std::io::Write;
use std::sync::mpsc;
use std::thread;

use vad::VadEngine;

const VAD_MODEL_PATH: &str = "models/silero_vad_v4.onnx";
const TARGET_RATE: usize = 16000;

#[hotpath::main]
fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Channel: audio -> VAD processor
    let (audio_tx, audio_rx) = hotpath::channel!(mpsc::channel::<Vec<f32>>());

    // Channel: VAD -> transcriber (finals only, preserved)
    let (final_tx, final_rx) = hotpath::channel!(mpsc::channel::<Vec<f32>>());

    // Channel: VAD -> transcriber (preview, lossy - sync_channel with capacity 1)
    let (preview_tx, preview_rx) = mpsc::sync_channel::<Vec<f32>>(1);

    // Start audio capture thread
    let _stream = audio::start_capture(audio_tx)?;

    // Start VAD processing thread
    let vad_handle = thread::spawn(move || {
        let vad = if std::path::Path::new(VAD_MODEL_PATH).exists() {
            match VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE) {
                Ok(v) => {
                    eprintln!("VAD: Silero enabled");
                    Some(v)
                }
                Err(e) => {
                    eprintln!("Silero VAD failed ({}), using energy-based", e);
                    Some(VadEngine::energy())
                }
            }
        } else {
            eprintln!("VAD model not found, using energy-based");
            Some(VadEngine::energy())
        };

        audio::run_vad_processor(audio_rx, final_tx, preview_tx, vad);
    });

    // Start transcription thread
    let transcribe_handle = thread::spawn(move || {
        let mut transcriber =
            match transcriber::Transcriber::new("models/parakeet-tdt-0.6b-v3-int8") {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Failed to load transcriber: {}", e);
                    return;
                }
            };

        let mut preview_text = String::new();

        loop {
            // Check for final (priority) or preview
            // Use try_recv for preview to make it non-blocking
            if let Ok(samples) = final_rx.try_recv() {
                while preview_rx.try_recv().is_ok() {}
                if let Ok(text) = transcriber.transcribe(&samples) {
                    if !text.is_empty() {
                        print!("\r\x1b[K{}\n", text);
                        std::io::stdout().flush().ok();
                    }
                }
                preview_text.clear();
                continue;
            }

            // Try preview (non-blocking)
            if let Ok(samples) = preview_rx.try_recv() {
                if samples.len() >= 8000 {
                    if let Ok(text) = transcriber.transcribe(&samples) {
                        if !text.is_empty() && text != preview_text {
                            preview_text = text.clone();
                            print!("\r\x1b[K\x1b[90m{}\x1b[0m", text);
                            std::io::stdout().flush().ok();
                        }
                    }
                }
                continue;
            }

            // Block on final if nothing available
            match final_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(samples) => {
                    if let Ok(text) = transcriber.transcribe(&samples) {
                        if !text.is_empty() {
                            print!("\r\x1b[K{}\n", text);
                            std::io::stdout().flush().ok();
                        }
                    }
                    preview_text.clear();
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    });

    println!("Listening... Press Ctrl+C to stop.\n");

    let _ = vad_handle.join();
    let _ = transcribe_handle.join();

    Ok(())
}
