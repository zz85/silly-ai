mod audio;
mod chat;
mod config;
mod render;
#[cfg(feature = "supertonic")]
mod supertonic;
mod transcriber;
mod tts;
mod ui;
mod vad;
mod wake;

use config::{Config, TtsConfig};
use render::{Renderer, Ui};

use clap::{Parser, Subcommand};
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use vad::VadEngine;

#[derive(Parser)]
#[command(name = "silly")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Transcription-only mode (no LLM/TTS)
    Transcribe,
}

const VAD_MODEL_PATH: &str = "models/silero_vad_v4.onnx";
const TARGET_RATE: usize = 16000;

#[hotpath::main]
fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main())
}

async fn async_main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let cli = Cli::parse();

    if matches!(cli.command, Some(Command::Transcribe)) {
        return run_transcribe_mode().await;
    }

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

    // Bridge std channel to tokio for async select
    let (async_display_tx, mut async_display_rx) =
        tokio::sync::mpsc::unbounded_channel::<DisplayEvent>();
    std::thread::spawn(move || {
        while let Ok(event) = display_rx.recv() {
            if async_display_tx.send(event).is_err() {
                break;
            }
        }
    });

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

        audio::run_vad_processor(
            audio_rx,
            final_tx,
            preview_tx,
            vad,
            tts_playing_vad,
            display_tx_audio,
        );
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

    // Load config and initialize TTS
    let config = Config::load();
    let tts_engine: tts::Tts = match config.tts {
        #[cfg(feature = "kokoro")]
        TtsConfig::Kokoro {
            model,
            voices,
            speed,
        } => {
            eprintln!("TTS: Kokoro (speed: {})", speed);
            let engine = tts::KokoroEngine::new(&model, &voices, speed).await;
            tts::Tts::new(Box::new(engine))
        }
        #[cfg(not(feature = "kokoro"))]
        TtsConfig::Kokoro { .. } => {
            panic!("Kokoro not enabled. Build with --features kokoro");
        }
        #[cfg(feature = "supertonic")]
        TtsConfig::Supertonic {
            onnx_dir,
            voice_style,
            speed,
        } => {
            eprintln!("TTS: Supertonic (speed: {})", speed);
            let engine = tts::SupertonicEngine::new(&onnx_dir, &voice_style, speed)
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
    if let Ok((stream, sink)) = tts::Tts::create_sink() {
        let _ = ollama_chat
            .greet_with_callback(
                |sentence| {
                    let _ = tts_engine.queue(sentence, &sink);
                },
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
        tts::Tts::finish(stream, sink);
    }
    tts_playing.store(false, Ordering::SeqCst);

    println!(
        "Listening for \"{}\"... (or type your message)\n",
        wake_word.phrase()
    );

    let (ui, ui_rx) = Ui::new();
    let mut renderer = Renderer::new();
    let mut last_interaction: Option<std::time::Instant> = None;
    let wake_timeout = std::time::Duration::from_secs(config.wake_timeout_secs);

    // Animation tick for spinners
    let ui_tick = ui.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            ui_tick.tick();
        }
    });

    // Channel for readline input and pre-fill requests
    let (input_tx, input_rx) = flume::unbounded::<String>();
    let (prefill_tx, prefill_rx) = flume::unbounded::<String>();

    // Readline thread
    thread::spawn(move || {
        use rustyline::DefaultEditor;
        let mut rl = DefaultEditor::new().expect("Failed to create readline");

        loop {
            // Check for prefill request
            let initial = prefill_rx.try_recv().unwrap_or_default();

            let result = if initial.is_empty() {
                rl.readline("> ")
            } else {
                rl.readline_with_initial("> ", (&initial, ""))
            };

            match result {
                Ok(line) => {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        let _ = rl.add_history_entry(&line);
                    }
                    if input_tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut pending_command: Option<String> = None;
    let mut pending_deadline: Option<tokio::time::Instant> = None;
    const EDIT_DELAY: std::time::Duration = std::time::Duration::from_millis(800);

    loop {
        let timeout_fut = async {
            match pending_deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending::<()>().await,
            }
        };

        tokio::select! {
            biased;

            Ok(line) = input_rx.recv_async() => {
                pending_command = None;
                pending_deadline = None;

                if line.is_empty() {
                    continue;
                }

                ui.show_final(&line);
                last_interaction = process_command(
                    &line, &tts_playing, &tts_engine, &mut ollama_chat, &ui
                ).await;
            }

            _ = timeout_fut, if pending_deadline.is_some() => {
                if let Some(command) = pending_command.take() {
                    pending_deadline = None;
                    // Send to readline as prefill
                    let _ = prefill_tx.send(command);
                }
            }

            event = async_display_rx.recv() => {
                match event {
                    Some(DisplayEvent::AudioLevel(_)) => {}
                    Some(DisplayEvent::Preview(text)) => {
                        if pending_command.is_none() {
                            ui.set_preview(text);
                        }
                    }
                    Some(DisplayEvent::Final(text)) => {
                        let in_conversation = last_interaction
                            .map(|t| t.elapsed() < wake_timeout)
                            .unwrap_or(false);

                        let command = if in_conversation {
                            text.clone()
                        } else {
                            match wake_word.detect(&text) {
                                Some(cmd) => cmd,
                                None => {
                                    ui.set_idle();
                                    continue;
                                }
                            }
                        };

                        if command.is_empty() {
                            ui.set_idle();
                            continue;
                        }

                        // Append to pending command buffer
                        if let Some(ref mut pending) = pending_command {
                            pending.push(' ');
                            pending.push_str(&command);
                        } else {
                            pending_command = Some(command);
                        }
                        pending_deadline = Some(tokio::time::Instant::now() + EDIT_DELAY);

                        ui.set_preview(format!("▶ {}", pending_command.as_ref().unwrap()));
                    }
                    None => break,
                }
            }

            Ok(ui_event) = ui_rx.recv_async() => {
                renderer.handle(ui_event);
            }
        }
    }

    let _ = vad_handle.join();
    let _ = preview_handle.join();
    let _ = final_handle.join();

    Ok(())
}

async fn process_command(
    command: &str,
    tts_playing: &Arc<AtomicBool>,
    tts_engine: &tts::Tts,
    ollama_chat: &mut chat::Chat,
    ui: &Ui,
) -> Option<std::time::Instant> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};

    tts_playing.store(true, Ordering::SeqCst);
    ui.set_thinking();

    let sink_result = tts::Tts::create_sink();
    match sink_result {
        Ok((stream, sink)) => {
            if let Err(e) = ollama_chat
                .send_streaming_with_callback(
                    command,
                    |sentence| {
                        let _ = tts_engine.queue(sentence, &sink);
                    },
                    || {},
                )
                .await
            {
                eprintln!("Chat error: {}", e);
            }

            ui.set_speaking();
            let mut paused = false;
            while !sink.empty() {
                // Check for keypress (non-blocking)
                if event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Char(' ') => {
                                    paused = !paused;
                                    if paused {
                                        sink.pause();
                                    } else {
                                        sink.play();
                                    }
                                }
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    sink.stop();
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            tts::Tts::finish(stream, sink);
        }
        Err(e) => {
            eprintln!("Audio error: {}", e);
            let _ = ollama_chat
                .send_streaming_with_callback(command, |_| {}, || {})
                .await;
        }
    }

    tts_playing.store(false, Ordering::SeqCst);
    ui.set_idle();
    Some(std::time::Instant::now())
}

enum DisplayEvent {
    Preview(String),
    Final(String),
    AudioLevel(f32),
}

async fn run_transcribe_mode() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
    let (final_tx, final_rx) = mpsc::channel::<Arc<[f32]>>();
    let (preview_tx, _) = mpsc::sync_channel::<Arc<[f32]>>(1); // unused but required
    let (display_tx, display_rx) = mpsc::channel::<DisplayEvent>();

    let _stream = audio::start_capture(audio_tx)?;

    let tts_playing = Arc::new(AtomicBool::new(false));
    let tts_playing_vad = Arc::clone(&tts_playing);

    thread::spawn(move || {
        let vad = if std::path::Path::new(VAD_MODEL_PATH).exists() {
            VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE).ok()
        } else {
            Some(VadEngine::energy())
        };
        audio::run_vad_processor(
            audio_rx,
            final_tx,
            preview_tx,
            vad,
            tts_playing_vad,
            display_tx,
        );
    });

    thread::spawn(move || {
        let mut transcriber =
            match transcriber::Transcriber::new("models/parakeet-tdt-0.6b-v3-int8") {
                Ok(t) => t,
                Err(_) => return,
            };
        while let Ok(samples) = final_rx.recv() {
            if let Ok(text) = transcriber.transcribe(&samples) {
                if !text.is_empty() {
                    println!("{}", text);
                }
            }
        }
    });

    eprintln!("Transcribe mode. Press Ctrl+C to stop.\n");

    loop {
        match display_rx.recv() {
            Ok(_) => {}
            Err(_) => break,
        }
    }

    Ok(())
}
