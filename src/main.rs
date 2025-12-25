mod audio;
mod chat;
mod config;
#[cfg(feature = "supertonic")]
mod supertonic;
mod transcriber;
mod tts;
mod ui;
mod vad;
mod wake;

use config::{Config, TtsConfig};

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
    let display_tx_audio = display_tx.clone();

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

        audio::run_vad_processor(audio_rx, final_tx, preview_tx, vad, tts_playing_vad, display_tx_audio);
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

    // Load config and initialize TTS
    let config = Config::load();
    let tts_engine: tts::Tts = match config.tts {
        #[cfg(feature = "kokoro")]
        TtsConfig::Kokoro { model, voices } => {
            eprintln!("TTS: Kokoro");
            let engine = tts::KokoroEngine::new(&model, &voices).await;
            tts::Tts::new(Box::new(engine))
        }
        #[cfg(not(feature = "kokoro"))]
        TtsConfig::Kokoro { .. } => {
            panic!("Kokoro not enabled. Build with --features kokoro");
        }
        #[cfg(feature = "supertonic")]
        TtsConfig::Supertonic { onnx_dir, voice_style } => {
            eprintln!("TTS: Supertonic");
            let engine = tts::SupertonicEngine::new(&onnx_dir, &voice_style)
                .expect("Failed to load Supertonic");
            tts::Tts::new(Box::new(engine))
        }
        #[cfg(not(feature = "supertonic"))]
        TtsConfig::Supertonic { .. } => {
            panic!("Supertonic not enabled. Build with --features supertonic");
        }
    };

    let mut ollama_chat = chat::Chat::new(&config.name);
    let wake_word = wake::WakeWord::new(&config.wake_word);

    // Initial greeting
    tts_playing.store(true, Ordering::SeqCst);
    if let Ok((_stream, sink)) = tts::Tts::create_sink() {
        let _ = ollama_chat
            .greet_with_callback(
                |sentence| { let _ = tts_engine.queue(sentence, &sink); },
                ui::thinking,
            )
            .await;
        let mut frame = 0;
        while !sink.empty() {
            ui::speaking(frame);
            frame += 1;
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        ui::clear_line();
    }
    tts_playing.store(false, Ordering::SeqCst);

    println!("Listening for \"{}\"... Press Ctrl+C to stop.\n", wake_word.phrase());

    let mut preview_text = String::new();
    let mut last_interaction: Option<std::time::Instant> = None;
    let wake_timeout = std::time::Duration::from_secs(config.wake_timeout_secs);

    loop {
        // Check for display events (non-blocking)
        match display_rx.try_recv() {
            Ok(DisplayEvent::AudioLevel(level)) => {
                if preview_text.is_empty() {
                    ui::show_level(level);
                }
            }
            Ok(DisplayEvent::Preview(text)) => {
                if text != preview_text {
                    preview_text = text.clone();
                    ui::show_preview(&text);
                }
            }
            Ok(DisplayEvent::Final(text)) => {
                // Check if within wake word timeout or need wake word
                let in_conversation = last_interaction
                    .map(|t| t.elapsed() < wake_timeout)
                    .unwrap_or(false);

                let command = if in_conversation {
                    text.clone()
                } else {
                    match wake_word.detect(&text) {
                        Some(cmd) => cmd,
                        None => {
                            preview_text.clear();
                            continue; // Ignore if no wake word
                        }
                    }
                };

                if command.is_empty() {
                    preview_text.clear();
                    continue; // Wake word only, no command
                }

                ui::show_final(&command);
                preview_text.clear();

                // Mute VAD during response
                tts_playing.store(true, Ordering::SeqCst);

                // Create sink for streaming TTS
                let sink_result = tts::Tts::create_sink();

                match sink_result {
                    Ok((_stream, sink)) => {
                        let tts = &tts_engine;
                        let sink_ref = &sink;

                        if let Err(e) = ollama_chat
                            .send_streaming_with_callback(
                                &command,
                                |sentence| {
                                    if let Err(e) = tts.queue(sentence, sink_ref) {
                                        eprintln!("TTS error: {}", e);
                                    }
                                },
                                || ui::thinking(),
                            )
                            .await
                        {
                            eprintln!("Chat error: {}", e);
                        }

                        // Wait for all queued audio to finish with animation
                        let mut frame = 0;
                        while !sink.empty() {
                            ui::speaking(frame);
                            frame += 1;
                            std::thread::sleep(std::time::Duration::from_millis(150));
                        }
                        ui::clear_line();
                    }
                    Err(e) => {
                        eprintln!("Audio output error: {}", e);
                        // Fallback: just stream text without TTS
                        if let Err(e) = ollama_chat
                            .send_streaming_with_callback(&command, |_| {}, || {})
                            .await
                        {
                            eprintln!("Chat error: {}", e);
                        }
                    }
                }

                // Update last interaction time after response completes
                last_interaction = Some(std::time::Instant::now());

                tts_playing.store(false, Ordering::SeqCst);
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
    AudioLevel(f32),
}
