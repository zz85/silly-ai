mod audio;
mod chat;
mod config;
mod render;
mod repl;
mod session;
#[cfg(feature = "supertonic")]
mod supertonic;
mod test_ui;
mod transcriber;
mod tts;
mod tui;
mod vad;
mod wake;

use config::{Config, TtsConfig};
use render::Ui;
use repl::TranscriptEvent;

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
    /// Test UI rendering without audio
    TestUi {
        /// Scene to test: idle, preview, thinking, speaking, response
        #[arg(default_value = "all")]
        scene: String,
    },
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

    match &cli.command {
        Some(Command::Transcribe) => return run_transcribe_mode().await,
        Some(Command::TestUi { scene }) => return test_ui::run(scene).await,
        None => {}
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

    let ollama_chat = chat::Chat::new(&config.name);
    let wake_word = wake::WakeWord::new(&config.wake_word);

    // Session manager channels
    let (session_tx, session_rx) = tokio::sync::mpsc::unbounded_channel::<session::SessionCommand>();
    let (session_event_tx, mut session_event_rx) = tokio::sync::mpsc::unbounded_channel::<session::SessionEvent>();
    
    // Spawn session manager
    let session_mgr = session::SessionManager::new(
        ollama_chat,
        tts_engine,
        Arc::clone(&tts_playing),
        session_event_tx,
    );
    // Spawn session manager on dedicated thread (OutputStream isn't Send)
    let session_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(session_mgr.run(session_rx));
    });

    let (ui, ui_rx) = Ui::new();

    // Initialize TUI
    let mut tui = tui::Tui::new()?;
    tui.draw()?;

    let mut last_interaction: Option<std::time::Instant> = None;
    let wake_timeout = std::time::Duration::from_secs(config.wake_timeout_secs);

    let mut auto_submit_deadline: Option<tokio::time::Instant> = None;

    // Initial greeting
    let _ = session_tx.send(session::SessionCommand::Greet);

    // Bridge ui_rx to async
    let (async_ui_tx, mut async_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let ui_rx_bridge = std::thread::spawn(move || {
        while let Ok(event) = ui_rx.recv() {
            if async_ui_tx.send(event).is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            // UI events from Ui sender
            Some(event) = async_ui_rx.recv() => {
                tui.handle_ui_event(event)?;
                tui.draw()?;
            }
            // Session events - process UI and draw immediately
            Some(event) = session_event_rx.recv() => {
                match event {
                    session::SessionEvent::Thinking => {
                        ui.set_thinking();
                    }
                    session::SessionEvent::Chunk(text) => {
                        ui.append_response(&text);
                    }
                    session::SessionEvent::ResponseEnd { response_words } => {
                        ui.end_response();
                        tui.set_last_response_words(response_words);
                    }
                    session::SessionEvent::Speaking => {
                        ui.set_speaking();
                    }
                    session::SessionEvent::SpeakingDone => {
                        ui.speaking_done();
                        last_interaction = Some(std::time::Instant::now());
                    }
                    session::SessionEvent::ContextWords(words) => {
                        ui.set_context_words(words);
                    }
                    session::SessionEvent::Ready => {
                        tui.set_ready();
                    }
                    session::SessionEvent::Error(e) => {
                        eprintln!("Session error: {}", e);
                        ui.set_idle();
                    }
                }
                // Process pending UI events and draw
                while let Ok(ui_event) = async_ui_rx.try_recv() {
                    tui.handle_ui_event(ui_event)?;
                }
                tui.draw()?;
            }
            // Audio transcription events
            Some(event) = async_display_rx.recv() => {
                match event {
                    DisplayEvent::AudioLevel(level) => {
                        tui.set_audio_level(level);
                    }
                    DisplayEvent::Preview(text) => {
                        repl::handle_transcript(
                            TranscriptEvent::Preview(text),
                            &wake_word,
                            last_interaction,
                            wake_timeout,
                            &ui,
                        );
                        // Cancel auto-submit on preview activity
                        auto_submit_deadline = None;
                    }
                    DisplayEvent::Final(text) => {
                        if let Some(input_text) = repl::handle_transcript(
                            TranscriptEvent::Final(text),
                            &wake_word,
                            last_interaction,
                            wake_timeout,
                            &ui,
                        ) {
                            tui.append_input(&input_text);
                            // Start/reset auto-submit timer
                            auto_submit_deadline = Some(tokio::time::Instant::now() + std::time::Duration::from_millis(1500));
                        }
                    }
                }
            }
            // Periodic: keyboard input, deadline check, redraw
            _ = tokio::time::sleep(std::time::Duration::from_millis(16)) => {
                // Poll keyboard input
                if let Some(line) = tui.poll_input()? {
                    if line == "\x03" {
                        break;
                    }
                    // Cancel auto-submit on manual submit
                    auto_submit_deadline = None;
                    ui.show_final(&line);
                    while let Ok(event) = async_ui_rx.try_recv() {
                        tui.handle_ui_event(event)?;
                    }
                    tui.draw()?;
                    let _ = session_tx.send(session::SessionCommand::UserInput(line));
                    continue;
                }

                // Cancel auto-submit timer on any keypress
                if tui.has_input_activity() {
                    auto_submit_deadline = None;
                }

                // Check auto-submit timeout
                if let Some(deadline) = auto_submit_deadline {
                    if tokio::time::Instant::now() >= deadline {
                        auto_submit_deadline = None;
                        if let Some(line) = tui.take_input() {
                            if !line.is_empty() {
                                ui.show_final(&line);
                                while let Ok(event) = async_ui_rx.try_recv() {
                                    tui.handle_ui_event(event)?;
                                }
                                tui.draw()?;
                                let _ = session_tx.send(session::SessionCommand::UserInput(line));
                            }
                        }
                    }
                }

                // Redraw
                tui.draw()?;
            }
        }
    }

    drop(tui);
    drop(ui_rx_bridge);

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
