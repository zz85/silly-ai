mod audio;
mod chat;
mod transcriber;
mod tts;
mod vad;

use std::error::Error;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

use vad::VadEngine;

const VAD_MODEL_PATH: &str = "models/silero_vad_v4.onnx";
const TARGET_RATE: usize = 16000;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main())
}

async fn async_main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Flag to mute VAD during TTS playback
    let tts_playing = Arc::new(AtomicBool::new(false));
    let tts_playing_vad = Arc::clone(&tts_playing);

    // Channel: audio -> VAD processor
    let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();

    // Channel: VAD -> final transcriber (preserved)
    let (final_tx, final_rx) = mpsc::channel::<Arc<[f32]>>();

    // Channel: VAD -> preview transcriber (lossy)
    let (preview_tx, preview_rx) = mpsc::sync_channel::<Arc<[f32]>>(1);

    // Channel: transcribers -> display
    let (display_tx, display_rx) = mpsc::channel::<DisplayEvent>();
    let display_tx2 = display_tx.clone();

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

        audio::run_vad_processor(audio_rx, final_tx, preview_tx, vad, tts_playing_vad);
    });

    // Preview transcription thread
    let preview_handle = thread::spawn(move || {
        let mut transcriber =
            match transcriber::Transcriber::new("models/parakeet-tdt-0.6b-v3-int8") {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Preview transcriber failed: {}", e);
                    return;
                }
            };

        while let Ok(samples) = preview_rx.recv() {
            if samples.len() >= 8000 {
                if let Ok(text) = transcriber.transcribe(&samples) {
                    if !text.is_empty() {
                        let _ = display_tx.send(DisplayEvent::Preview(text));
                    }
                }
            }
        }
    });

    // Final transcription thread
    let final_handle = thread::spawn(move || {
        let mut transcriber =
            match transcriber::Transcriber::new("models/parakeet-tdt-0.6b-v3-int8") {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Final transcriber failed: {}", e);
                    return;
                }
            };

        while let Ok(samples) = final_rx.recv() {
            if let Ok(text) = transcriber.transcribe(&samples) {
                if !text.is_empty() {
                    let _ = display_tx2.send(DisplayEvent::Final(text));
                }
            }
        }
    });

    // Channel for chat responses
    let (chat_tx, chat_rx) = mpsc::channel::<String>();

    // Initialize TTS
    let tts_engine = tts::Tts::new("models/kokoro-v1.0.onnx", "models/voices-v1.0.bin").await;

    let mut ollama_chat = chat::Chat::new();

    // Initial greeting
    tts_playing.store(true, Ordering::SeqCst);
    if let Ok(greeting) = ollama_chat.greet().await {
        if let Err(e) = tts_engine.speak(&greeting) {
            eprintln!("TTS error: {}", e);
        }
    }
    tts_playing.store(false, Ordering::SeqCst);

    println!("Listening... Press Ctrl+C to stop.\n");

    let mut preview_text = String::new();

    loop {
        // Check for display events (non-blocking)
        match display_rx.try_recv() {
            Ok(DisplayEvent::Preview(text)) => {
                if text != preview_text {
                    preview_text = text.clone();
                    print!("\r\x1b[K\x1b[90m{}\x1b[0m", text);
                    std::io::stdout().flush().ok();
                }
            }
            Ok(DisplayEvent::Final(text)) => {
                print!("\r\x1b[K> {}\n", text);
                std::io::stdout().flush().ok();
                preview_text.clear();

                // Send to ollama (streaming)
                match ollama_chat.send_streaming(&text).await {
                    Ok(response) => {
                        // Mute VAD during TTS
                        tts_playing.store(true, Ordering::SeqCst);
                        if let Err(e) = tts_engine.speak(&response) {
                            eprintln!("TTS error: {}", e);
                        }
                        tts_playing.store(false, Ordering::SeqCst);
                    }
                    Err(e) => {
                        eprintln!("Chat error: {}", e);
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
    }

    let _ = vad_handle.join();
    let _ = preview_handle.join();
    let _ = final_handle.join();

    Ok(())
}

enum DisplayEvent {
    Preview(String),
    Final(String),
}
