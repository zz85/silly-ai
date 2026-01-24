mod audio;
#[cfg(feature = "listen")]
mod capture;
mod chat;
mod command;
mod config;
mod fuzzy;
mod graphical_ui;
#[cfg(feature = "listen")]
mod listen;
mod llm;
#[cfg(feature = "listen")]
mod pipeline;
mod render;
mod repl;
#[cfg(feature = "listen")]
mod segmenter;
mod session;
mod state;
mod stats;
mod status_bar;
#[cfg(feature = "listen")]
mod summarize;
#[cfg(feature = "supertonic")]
mod supertonic;
mod test_ui;
mod transcriber;
mod tts;
mod tui;
mod vad;
mod wake;

use command::{CommandProcessor, CommandResult};
use config::{Config, LlmConfig, OrbStyleConfig, TtsConfig, UiModeConfig};
use render::{OrbStyle, Ui, UiEvent, UiMode, UiRenderer};
use repl::{TranscriptEvent, TranscriptResult};
use state::RuntimeState;

use clap::{Parser, Subcommand};
use std::error::Error;
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(feature = "listen")]
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use vad::VadEngine;

fn debug_log(msg: &str) {
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug.log")
    {
        let _ = writeln!(
            file,
            "{}: {}",
            chrono::Utc::now().format("%H:%M:%S%.3f"),
            msg
        );
    }
}

#[derive(Parser)]
#[command(name = "silly")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Disable speech-to-text (type input only)
    #[arg(long)]
    no_stt: bool,

    /// Disable text-to-speech output
    #[arg(long)]
    no_tts: bool,

    /// Use orb visualization UI instead of text UI
    #[arg(long, short = 'o')]
    orb: bool,

    /// Use text UI (default, overrides config)
    #[arg(long, short = 't')]
    text: bool,

    /// Visual style for graphical UI: orbs, blob, or ring
    #[arg(long, value_parser = ["orbs", "blob", "ring"])]
    orb_style: Option<String>,
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
    /// Demo graphical orb animations (all states and styles)
    OrbDemo,
    /// Capture and transcribe audio continuously
    #[cfg(feature = "listen")]
    Listen {
        /// Audio source: mic, system, or app name
        #[arg(short, long)]
        source: Option<String>,
        /// Output file for transcription
        #[arg(short, long, default_value = "transcript.txt")]
        output: PathBuf,
        /// List available applications
        #[arg(long)]
        list: bool,
        /// Save raw audio to WAV file for debugging
        #[arg(long)]
        debug_wav: Option<PathBuf>,
        /// Save compressed audio to OGG file (64kbps)
        #[arg(long)]
        save_ogg: Option<PathBuf>,
        /// Multi-source mode: capture from two sources with attribution
        #[arg(long)]
        multi: bool,
    },
    /// Record audio to OGG file (no transcription)
    #[cfg(feature = "listen")]
    Record {
        /// Audio source: mic, system, or app name
        #[arg(short, long)]
        source: Option<String>,
        /// Output OGG file
        #[arg(short, long, default_value = "recording.ogg")]
        output: PathBuf,
        /// List available applications
        #[arg(long)]
        list: bool,
    },
    /// Summarize a transcription file using LLM
    #[cfg(feature = "listen")]
    Summarize {
        /// Input transcription file
        #[arg(short, long)]
        input: PathBuf,
    },
    /// Transcribe a WAV file (for debugging)
    #[cfg(feature = "listen")]
    TranscribeWav {
        /// Input WAV file
        #[arg(short, long)]
        input: PathBuf,
    },
    /// Quick test of LLM backend
    Probe {
        /// Question to ask
        prompt: String,
    },
}

const VAD_MODEL_PATH: &str = "models/silero_vad_v4.onnx";
const TARGET_RATE: usize = 16000;

#[hotpath::main]
fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let cli = Cli::parse();

    // Handle sync commands before starting async runtime
    #[cfg(feature = "listen")]
    if let Some(Command::Summarize { input }) = &cli.command {
        return summarize::run_summarize(input.clone());
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main_with_cli(cli))
}

async fn async_main_with_cli(cli: Cli) -> Result<(), Box<dyn Error + Send + Sync>> {
    match &cli.command {
        Some(Command::Transcribe) => return run_transcribe_mode().await,
        Some(Command::TestUi { scene }) => return test_ui::run(scene).await,
        Some(Command::OrbDemo) => {
            return graphical_ui::run_orb_demo()
                .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>);
        }
        #[cfg(feature = "listen")]
        Some(Command::Listen {
            source,
            output,
            list,
            debug_wav,
            save_ogg,
            multi,
        }) => {
            if *list {
                return listen::list_apps();
            }
            if *multi {
                let (src1, src2) = listen::pick_sources_multi()?;
                return listen::run_multi_source(src1, src2, output.clone());
            }
            let src = match source {
                Some(s) if s == "mic" => listen::AudioSource::Mic,
                Some(s) if s == "system" => listen::AudioSource::System,
                Some(s) => listen::AudioSource::App(s.clone()),
                None => listen::pick_source_interactive()?,
            };
            return listen::run_listen(src, output.clone(), debug_wav.clone(), save_ogg.clone());
        }
        #[cfg(feature = "listen")]
        Some(Command::Record {
            source,
            output,
            list,
        }) => {
            if *list {
                return listen::list_apps();
            }
            let src = match source {
                Some(s) if s == "mic" => listen::AudioSource::Mic,
                Some(s) if s == "system" => listen::AudioSource::System,
                Some(s) => listen::AudioSource::App(s.clone()),
                None => listen::pick_source_interactive()?,
            };
            return pipeline::run_record_only(src, output.clone());
        }
        #[cfg(feature = "listen")]
        Some(Command::Summarize { .. }) => unreachable!("handled in main()"),
        #[cfg(feature = "listen")]
        Some(Command::TranscribeWav { input }) => {
            return listen::transcribe_wav(input.clone());
        }
        Some(Command::Probe { prompt }) => {
            return run_probe(prompt).await;
        }
        None => {}
    }

    // Load config early for acceleration settings
    let config = Config::load();

    // Create shared runtime state
    let runtime_state = RuntimeState::new(&config);

    // Apply CLI flags to runtime state
    if cli.no_stt {
        runtime_state.mic_muted.store(true, Ordering::SeqCst);
        runtime_state.wake_enabled.store(false, Ordering::SeqCst);
    }
    if cli.no_tts {
        runtime_state.tts_enabled.store(false, Ordering::SeqCst);
    }

    // Create command processor
    let command_processor = CommandProcessor::new(&config);

    // Shared stats for performance tracking
    let stats = stats::new_shared();
    let stats_transcribe = Arc::clone(&stats);
    let stats_tts = Arc::clone(&stats);
    let stats_session = Arc::clone(&stats);

    // Legacy flags for backward compatibility (VAD processor and transcribe mode)
    let mic_muted = Arc::new(AtomicBool::new(cli.no_stt));
    let tts_enabled = Arc::new(AtomicBool::new(!cli.no_tts));
    let wake_enabled = Arc::new(AtomicBool::new(!cli.no_stt));

    // Clone runtime_state for VAD processor
    let runtime_state_vad = Arc::clone(&runtime_state);

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

    // TTS level monitor thread - send updates when TTS is playing
    let runtime_state_tts = Arc::clone(&runtime_state);
    let display_tx_tts = display_tx.clone();
    thread::spawn(move || {
        loop {
            // Only send if TTS is playing
            if runtime_state_tts.tts_playing.load(Ordering::SeqCst) {
                // Read real-time TTS level from runtime state (updated by MonitoredSource)
                let level = runtime_state_tts.get_tts_level();
                let _ = display_tx_tts.send(DisplayEvent::TtsLevel(level));
            }
            // Update every 50ms (same as audio level)
            thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    // Start VAD processing thread with crosstalk support
    let vad_handle = thread::spawn(move || {
        let use_gpu_vad = config.acceleration.vad_gpu;

        let vad = if std::path::Path::new(VAD_MODEL_PATH).exists() {
            #[cfg(all(feature = "supertonic", target_arch = "aarch64", target_os = "macos"))]
            let vad_result = if use_gpu_vad {
                VadEngine::silero_with_gpu(VAD_MODEL_PATH, TARGET_RATE)
            } else {
                VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE)
            };

            #[cfg(not(all(feature = "supertonic", target_arch = "aarch64", target_os = "macos")))]
            let vad_result = VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE);

            match vad_result {
                Ok(v) => {
                    eprintln!(
                        "VAD: Silero enabled (crosstalk: {})",
                        runtime_state_vad.crosstalk_enabled.load(Ordering::SeqCst)
                    );
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

        // Use the new crosstalk-enabled VAD processor
        if let Some(vad_engine) = vad {
            audio::run_vad_processor_with_state(
                audio_rx,
                final_tx,
                preview_tx,
                Some(vad_engine),
                runtime_state_vad,
                display_tx_audio,
            );
        } else {
            eprintln!("Failed to initialize VAD engine");
            // Continue with energy-based VAD as fallback
            audio::run_vad_processor_with_state(
                audio_rx,
                final_tx,
                preview_tx,
                Some(VadEngine::energy()),
                runtime_state_vad,
                display_tx_audio,
            );
        }
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
        let mut transcriber = match transcriber::Transcriber::with_stats(
            "models/parakeet-tdt-0.6b-v3-int8",
            Some(stats_transcribe),
        ) {
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

    // Initialize TTS (config already loaded above)
    let use_gpu_tts = config.acceleration.tts_gpu;
    let tts_engine: tts::Tts = match config.tts {
        #[cfg(feature = "kokoro")]
        TtsConfig::Kokoro {
            model,
            voices,
            speed,
        } => {
            eprintln!("TTS: Kokoro (speed: {})", speed);
            let engine = tts::KokoroEngine::new(&model, &voices, speed).await;
            tts::Tts::with_stats(Box::new(engine), stats_tts)
        }
        #[cfg(not(feature = "kokoro"))]
        TtsConfig::Kokoro { .. } => {
            eprintln!("Warning: Kokoro not enabled. Build with --features kokoro");
            // Fallback to Supertonic if available
            #[cfg(feature = "supertonic")]
            {
                eprintln!("Falling back to Supertonic TTS");
                let engine = tts::SupertonicEngine::new(
                    "models/supertonic/onnx",
                    "models/supertonic/voice_styles/M1.json",
                    1.1,
                    use_gpu_tts,
                )
                .unwrap_or_else(|e| {
                    eprintln!("Failed to initialize Supertonic TTS: {}", e);
                    panic!("No working TTS engine available");
                });
                tts::Tts::with_stats(Box::new(engine), stats_tts)
            }
            #[cfg(not(feature = "supertonic"))]
            {
                panic!("Kokoro not enabled. Build with --features kokoro");
            }
        }
        #[cfg(feature = "supertonic")]
        TtsConfig::Supertonic {
            onnx_dir,
            voice_style,
            speed,
        } => {
            eprintln!("TTS: Supertonic (speed: {}, GPU: {})", speed, use_gpu_tts);
            let engine = tts::SupertonicEngine::new(&onnx_dir, &voice_style, speed, use_gpu_tts)
                .map_err(|e| {
                    eprintln!("Failed to load Supertonic TTS: {}", e);
                    "Supertonic TTS initialization failed"
                })?;
            tts::Tts::with_stats(Box::new(engine), stats_tts)
        }
        #[cfg(not(feature = "supertonic"))]
        TtsConfig::Supertonic { .. } => {
            eprintln!("Warning: Supertonic not enabled. Build with --features supertonic");
            // Fallback to Kokoro if available
            #[cfg(feature = "kokoro")]
            {
                eprintln!("Falling back to Kokoro TTS");
                let engine = tts::KokoroEngine::new(
                    "models/kokoro-v1.0.onnx",
                    "models/voices-v1.0.bin",
                    1.1,
                )
                .await;
                tts::Tts::with_stats(Box::new(engine), stats_tts)
            }
            #[cfg(not(feature = "kokoro"))]
            {
                panic!("Supertonic not enabled. Build with --features supertonic");
            }
        }
    };

    // Initialize LLM backend
    let system_prompt = chat::system_prompt(&config.name);
    let llm_backend: Box<dyn llm::LlmBackend> = match config.llm {
        #[cfg(feature = "llama-cpp")]
        LlmConfig::LlamaCpp {
            model_path,
            hf_repo,
            hf_file,
            prompt_format,
            ctx_size,
        } => {
            let backend = if let Some(path) = model_path {
                llm::llama::LlamaCppBackend::from_path(
                    path,
                    &system_prompt,
                    prompt_format,
                    ctx_size,
                )?
            } else {
                llm::llama::LlamaCppBackend::from_hf(
                    &hf_repo,
                    &hf_file,
                    &system_prompt,
                    prompt_format,
                    ctx_size,
                )?
            };
            Box::new(backend)
        }
        #[cfg(not(feature = "llama-cpp"))]
        LlmConfig::LlamaCpp { .. } => {
            panic!("llama-cpp not enabled. Build with --features llama-cpp");
        }
        #[cfg(feature = "ollama")]
        LlmConfig::Ollama { model } => {
            Box::new(llm::ollama::OllamaBackend::new(&model, &system_prompt))
        }
        #[cfg(not(feature = "ollama"))]
        LlmConfig::Ollama { .. } => {
            panic!("Ollama not enabled. Build with --features ollama");
        }
        #[cfg(feature = "openai-compat")]
        LlmConfig::OpenAiCompat {
            ref base_url,
            ref model,
            ref api_key,
            temperature,
            top_p,
            max_tokens,
            presence_penalty,
            frequency_penalty,
            ..
        } => Box::new(llm::openai_compat::OpenAiCompatBackend::new(
            base_url.clone(),
            model.clone(),
            api_key.clone(),
            temperature,
            top_p,
            max_tokens,
            presence_penalty,
            frequency_penalty,
        )?),
        #[cfg(not(feature = "openai-compat"))]
        LlmConfig::OpenAiCompat { .. } => {
            panic!("OpenAI-compatible backend not enabled. Build with --features openai-compat");
        }
        #[cfg(feature = "kalosm")]
        LlmConfig::Kalosm { ref model } => {
            use kalosm_llama::LlamaSource;
            let source = match model.as_str() {
                "phi3" => LlamaSource::phi_3_mini_4k_instruct(),
                "llama3-8b" => LlamaSource::llama_3_8b_chat(),
                "mistral-7b" => LlamaSource::mistral_7b_instruct_2(),
                "qwen-0.5b" => LlamaSource::qwen_0_5b_chat(),
                "qwen-1.5b" => LlamaSource::qwen_1_5b_chat(),
                _ => LlamaSource::qwen_1_5b_chat(),
            };
            Box::new(llm::kalosm_backend::KalosmBackend::new_blocking(
                source,
                &system_prompt,
            )?)
        }
        #[cfg(not(feature = "kalosm"))]
        LlmConfig::Kalosm { .. } => {
            panic!("Kalosm not enabled. Build with --features kalosm");
        }
    };

    let llm_chat = chat::Chat::new(llm_backend);
    let wake_word = wake::WakeWord::new(&config.wake_word);

    // Session manager channels
    let (session_tx, session_rx) =
        tokio::sync::mpsc::unbounded_channel::<session::SessionCommand>();
    let (session_event_tx, mut session_event_rx) =
        tokio::sync::mpsc::unbounded_channel::<session::SessionEvent>();

    // Spawn session manager
    let session_mgr = session::SessionManager::new(
        llm_chat,
        tts_engine,
        Arc::clone(&runtime_state),
        session_event_tx,
    )
    .with_stats(stats_session);
    // Spawn session manager on dedicated thread (LLM inference is blocking)
    let _session_handle = std::thread::spawn(move || {
        session_mgr.run_sync(session_rx);
    });

    let (ui, ui_rx) = Ui::new();

    // Determine UI mode from CLI flags or config
    let ui_mode = if cli.text {
        UiModeConfig::Text
    } else if cli.orb {
        UiModeConfig::Orb
    } else {
        config.ui.mode
    };

    // Determine orb style
    let orb_style = match cli.orb_style.as_deref() {
        Some("ring") => OrbStyle::Ring,
        Some("blob") => OrbStyle::Blob,
        Some("orbs") => OrbStyle::Orbs,
        _ => match config.ui.orb_style {
            OrbStyleConfig::Ring => OrbStyle::Ring,
            OrbStyleConfig::Blob => OrbStyle::Blob,
            OrbStyleConfig::Orbs => OrbStyle::Orbs,
        },
    };

    // Initialize UI based on mode
    let mut ui_renderer: Box<dyn UiRenderer> = match ui_mode {
        UiModeConfig::Text => Box::new(tui::Tui::new()?),
        UiModeConfig::Orb => {
            let mut gui = graphical_ui::GraphicalUi::new()?;
            gui.set_visual_style(orb_style);
            Box::new(gui)
        }
    };
    ui_renderer.draw()?;

    let mut last_interaction: Option<std::time::Instant> = None;
    let wake_timeout = std::time::Duration::from_secs(config.wake_timeout_secs);

    let auto_submit_delay = std::time::Duration::from_millis(2000);
    let mut auto_submit_deadline: Option<tokio::time::Instant> = None;

    // Temporary mic mute on keypress
    let keypress_mute_duration = std::time::Duration::from_secs(1);
    let mut keypress_mute_until: Option<std::time::Instant> = None;

    // Initial greeting
    let _ = session_tx.send(session::SessionCommand::Greet);

    // Bridge ui_rx to async
    let (async_ui_tx, mut async_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let ui_rx_bridge = std::thread::spawn(move || {
        while let Ok(event) = ui_rx.recv() {
            debug_log(&format!("UI bridge received event: {:?}", event));
            if async_ui_tx.send(event).is_err() {
                debug_log("Failed to send event through async channel");
                break;
            }
            debug_log("Event sent through async channel");
        }
        debug_log("UI bridge thread exiting");
    });

    loop {
        tokio::select! {
            // UI events from Ui sender
            Some(event) = async_ui_rx.recv() => {
                // Check for UI mode switching events
                if let UiEvent::SwitchUiMode(new_mode) = &event {
                    debug_log(&format!("Received SwitchUiMode event: {:?}", new_mode));
                    let current_mode = ui_renderer.ui_mode();
                    debug_log(&format!("Current UI mode: {:?}", current_mode));
                    if *new_mode != current_mode {
                        debug_log(&format!("Switching UI mode from {:?} to {:?}", current_mode, new_mode));
                        // Restore terminal state from old UI
                        ui_renderer.restore()?;

                        // Create new UI renderer
                        ui_renderer = match *new_mode {
                            UiMode::Text => {
                                debug_log("Creating new text UI");
                                let new_tui = Box::new(tui::Tui::new()?);
                                debug_log("Text UI created successfully");
                                new_tui
                            }
                            UiMode::Orb => {
                                debug_log("Creating new orb UI");
                                let mut gui = graphical_ui::GraphicalUi::new()?;
                                gui.set_visual_style(orb_style);
                                debug_log("Orb UI created successfully");
                                Box::new(gui)
                            }
                        };

                        // Sync state with new UI
                        ui_renderer.set_mic_muted(runtime_state.mic_muted.load(Ordering::SeqCst));
                        ui_renderer.set_tts_enabled(runtime_state.tts_enabled.load(Ordering::SeqCst));
                        ui_renderer.set_wake_enabled(runtime_state.wake_enabled.load(Ordering::SeqCst));
                        ui_renderer.set_mode(runtime_state.mode());

                        // Force an immediate draw to ensure UI is visible
                        ui_renderer.draw()?;
                        debug_log("UI switch completed");
                    } else {
                        debug_log("UI mode already matches, no switch needed");
                    }
                } else {
                    ui_renderer.handle_ui_event(event)?;
                }
                ui_renderer.draw()?;
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
                        ui_renderer.set_last_response_words(response_words);
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
                        ui_renderer.set_ready();
                    }
                    session::SessionEvent::Error(e) => {
                        ui.show_error(&e);
                        ui.set_idle();
                    }
                }
                // Process pending UI events and draw
                while let Ok(ui_event) = async_ui_rx.try_recv() {
                    ui_renderer.handle_ui_event(ui_event)?;
                }
                ui_renderer.draw()?;
            }
            // Audio transcription events - mode-aware handling
            Some(event) = async_display_rx.recv() => {
                match event {
                    DisplayEvent::AudioLevel(level) => {
                        ui_renderer.set_audio_level(level);
                    }
                    DisplayEvent::TtsLevel(level) => {
                        ui_renderer.set_tts_level(level);
                    }
                    DisplayEvent::Preview(text) => {
                        // Use mode-aware transcript handling
                        let _result = repl::handle_transcript_with_mode(
                            TranscriptEvent::Preview(text),
                            &wake_word,
                            last_interaction,
                            wake_timeout,
                            &runtime_state,
                            &command_processor,
                            &ui,
                        );
                        // Preview events mean user is still speaking - cancel auto-submit timer
                        // IMPORTANT: This must ALWAYS cancel, regardless of result value
                        // See docs/auto_submit_timer.md for rationale
                        auto_submit_deadline = None;
                    }
                    DisplayEvent::Final(text) => {
                        // Use mode-aware transcript handling
                        let result = repl::handle_transcript_with_mode(
                            TranscriptEvent::Final(text),
                            &wake_word,
                            last_interaction,
                            wake_timeout,
                            &runtime_state,
                            &command_processor,
                            &ui,
                        );

                        match result {
                            TranscriptResult::SendToLlm(input_text) => {
                                ui_renderer.append_input(&input_text);
                                // Start/restart auto-submit timer with fresh deadline
                                // IMPORTANT: This must set a NEW deadline, not check if one exists
                                // See docs/auto_submit_timer.md for rationale
                                auto_submit_deadline = Some(tokio::time::Instant::now() + auto_submit_delay);
                            }
                            TranscriptResult::TranscribeOnly(text) => {
                                // Transcribe mode: just display the text, no LLM
                                ui_renderer.show_message(&format!("[Transcribed] {}", text));
                            }
                            TranscriptResult::AppendNote(text) => {
                                // Note-taking mode: append to notes file
                                if let Err(e) = repl::append_to_notes(&text) {
                                    ui_renderer.show_message(&format!("Failed to save note: {}", e));
                                } else {
                                    ui_renderer.show_message(&format!("[Note saved] {}", text));
                                }
                            }
                            TranscriptResult::CommandHandled(msg) => {
                                // Command was handled
                                if let Some(m) = msg {
                                    ui_renderer.show_message(&m);
                                }
                                // Sync legacy flags with runtime state
                                mic_muted.store(runtime_state.mic_muted.load(Ordering::SeqCst), Ordering::SeqCst);
                                tts_enabled.store(runtime_state.tts_enabled.load(Ordering::SeqCst), Ordering::SeqCst);
                                wake_enabled.store(runtime_state.wake_enabled.load(Ordering::SeqCst), Ordering::SeqCst);
                                ui_renderer.set_mic_muted(runtime_state.mic_muted.load(Ordering::SeqCst));
                                ui_renderer.set_tts_enabled(runtime_state.tts_enabled.load(Ordering::SeqCst));
                                ui_renderer.set_wake_enabled(runtime_state.wake_enabled.load(Ordering::SeqCst));
                            }
                            TranscriptResult::Stop => {
                                let _ = session_tx.send(session::SessionCommand::Cancel);
                            }
                            TranscriptResult::Shutdown => {
                                break;
                            }
                            TranscriptResult::ModeChange { mode, announcement } => {
                                runtime_state.set_mode(mode);
                                ui_renderer.set_mode(mode);
                                if let Some(msg) = announcement {
                                    ui_renderer.show_message(&msg);
                                }
                            }
                            TranscriptResult::None => {
                                // No action needed
                            }
                        }
                    }
                }
            }
            // Periodic: keyboard input, deadline check, redraw
            _ = tokio::time::sleep(std::time::Duration::from_millis(16)) => {
                // Poll keyboard input - drain all available events before redrawing
                let mut should_break = false;
                loop {
                    match ui_renderer.poll_input()? {
                        None => {
                            debug_log("Main: No keyboard input available");
                            break;
                        }
                        Some(line) => {
                            debug_log(&format!("Main: Keyboard input received: {}", line));

                            // Handle Ctrl+C
                            if line == "\x03" {
                                should_break = true;
                                break;
                            }

                            // Check for slash commands first
                            if let Some(cmd_result) = command::process_slash_command(&line, &runtime_state) {
                                match cmd_result {
                                    CommandResult::Handled(Some(msg)) => {
                                        debug_log(&format!("Command result: {}", msg));
                                        // Check for UI switching commands
                                        if msg.starts_with("ui_switch:") {
                                            let new_mode = &msg[10..];
                                            debug_log(&format!("UI switch requested to: {}", new_mode));
                                            match new_mode {
                                                "text" => {
                                                    debug_log("Requesting switch to text UI");
                                                    ui.request_ui_mode_switch(UiMode::Text);
                                                    ui_renderer.show_message("Switching to text UI...");
                                                }
                                                "orb" => {
                                                    debug_log("Requesting switch to orb UI");
                                                    ui.request_ui_mode_switch(UiMode::Orb);
                                                    ui_renderer.show_message("Switching to orb UI...");
                                                }
                                                "toggle" => {
                                                    let current = ui_renderer.ui_mode();
                                                    let new = match current {
                                                        UiMode::Text => UiMode::Orb,
                                                        UiMode::Orb => UiMode::Text,
                                                    };
                                                    debug_log(&format!("Toggling UI from {:?} to {:?}", current, new));
                                                    ui.request_ui_mode_switch(new);
                                                }
                                                _ => {
                                                    ui_renderer.show_message(&msg);
                                                }
                                            }
                                        } else {
                                            ui_renderer.show_message(&msg);
                                        }
                                    }
                                    CommandResult::Handled(None) => {}
                                    CommandResult::Stop => {
                                        let _ = session_tx.send(session::SessionCommand::Cancel);
                                    }
                                    CommandResult::Shutdown => {
                                        should_break = true;
                                        break;
                                    }
                                    CommandResult::ModeChange { mode, announcement } => {
                                        runtime_state.set_mode(mode);
                                        ui_renderer.set_mode(mode);
                                        if let Some(msg) = announcement {
                                            ui_renderer.show_message(&msg);
                                        }
                                    }
                                    CommandResult::PassThrough(_) => {}
                                }
                                // Sync legacy flags with runtime state
                                mic_muted.store(runtime_state.mic_muted.load(Ordering::SeqCst), Ordering::SeqCst);
                                tts_enabled.store(runtime_state.tts_enabled.load(Ordering::SeqCst), Ordering::SeqCst);
                                wake_enabled.store(runtime_state.wake_enabled.load(Ordering::SeqCst), Ordering::SeqCst);
                                ui_renderer.set_mic_muted(runtime_state.mic_muted.load(Ordering::SeqCst));
                                ui_renderer.set_tts_enabled(runtime_state.tts_enabled.load(Ordering::SeqCst));
                                ui_renderer.set_wake_enabled(runtime_state.wake_enabled.load(Ordering::SeqCst));
                                keypress_mute_until = None;
                                continue;
                            }

                            // Handle /stats separately (not in command module)
                            if line == "/stats" {
                                let summary = stats.lock().unwrap().summary();
                                ui_renderer.show_message(&summary);
                                continue;
                            }

                            // Process through command processor for voice-style commands
                            let cmd_result = command_processor.process(&line, &runtime_state);
                            match cmd_result {
                                CommandResult::Stop => {
                                    let _ = session_tx.send(session::SessionCommand::Cancel);
                                    continue;
                                }
                                CommandResult::Shutdown => {
                                    should_break = true;
                                    break;
                                }
                                CommandResult::Handled(Some(msg)) => {
                                    ui_renderer.show_message(&msg);
                                    // Sync legacy flags
                                    mic_muted.store(runtime_state.mic_muted.load(Ordering::SeqCst), Ordering::SeqCst);
                                    tts_enabled.store(runtime_state.tts_enabled.load(Ordering::SeqCst), Ordering::SeqCst);
                                    wake_enabled.store(runtime_state.wake_enabled.load(Ordering::SeqCst), Ordering::SeqCst);
                                    continue;
                                }
                                CommandResult::Handled(None) => {
                                    // Sync legacy flags
                                    mic_muted.store(runtime_state.mic_muted.load(Ordering::SeqCst), Ordering::SeqCst);
                                    tts_enabled.store(runtime_state.tts_enabled.load(Ordering::SeqCst), Ordering::SeqCst);
                                    wake_enabled.store(runtime_state.wake_enabled.load(Ordering::SeqCst), Ordering::SeqCst);
                                    continue;
                                }
                                CommandResult::ModeChange { mode, announcement } => {
                                    runtime_state.set_mode(mode);
                                    ui_renderer.set_mode(mode);
                                    if let Some(msg) = announcement {
                                        ui_renderer.show_message(&msg);
                                    }
                                    continue;
                                }
                                CommandResult::PassThrough(text) => {
                                    // Cancel auto-submit on manual submit
                                    auto_submit_deadline = None;
                                    // Cancel any in-progress response
                                    let _ = session_tx.send(session::SessionCommand::Cancel);
                                    ui.show_final(&text);
                                    let _ = session_tx.send(session::SessionCommand::UserInput(text));
                                    break;
                                }
                            }
                        }
                    }
                }
                if should_break {
                    break;
                }

                // Process pending UI events and draw
                while let Ok(ui_event) = async_ui_rx.try_recv() {
                    // Check for UI mode switching events in the periodic branch too
                    if let UiEvent::SwitchUiMode(new_mode) = &ui_event {
                        debug_log(&format!("Received SwitchUiMode event in periodic branch: {:?}", new_mode));
                        let current_mode = ui_renderer.ui_mode();
                        debug_log(&format!("Current UI mode: {:?}", current_mode));
                        if *new_mode != current_mode {
                            debug_log(&format!("Switching UI mode from {:?} to {:?}", current_mode, new_mode));
                            // Restore terminal state from old UI
                            ui_renderer.restore()?;

                            // Create new UI renderer
                            ui_renderer = match *new_mode {
                                UiMode::Text => {
                                    debug_log("Creating new text UI");
                                    let new_tui = Box::new(tui::Tui::new()?);
                                    debug_log("Text UI created successfully");
                                    new_tui
                                }
                                UiMode::Orb => {
                                    debug_log("Creating new orb UI");
                                    let mut gui = graphical_ui::GraphicalUi::new()?;
                                    gui.set_visual_style(orb_style);
                                    debug_log("Orb UI created successfully");
                                    Box::new(gui)
                                }
                            };

                            // Sync state with new UI
                            ui_renderer.set_mic_muted(runtime_state.mic_muted.load(Ordering::SeqCst));
                            ui_renderer.set_tts_enabled(runtime_state.tts_enabled.load(Ordering::SeqCst));
                            ui_renderer.set_wake_enabled(runtime_state.wake_enabled.load(Ordering::SeqCst));
                            ui_renderer.set_mode(runtime_state.mode());

                            // Force an immediate draw to ensure UI is visible
                            ui_renderer.draw()?;
                            debug_log("UI switch completed in periodic branch");
                        } else {
                            debug_log("UI mode already matches, no switch needed");
                        }
                    } else {
                        ui_renderer.handle_ui_event(ui_event)?;
                    }
                }

                // Temporarily mute mic on any keypress
                if ui_renderer.has_keypress_activity() {
                    mic_muted.store(true, Ordering::SeqCst);
                    runtime_state.mic_muted.store(true, Ordering::SeqCst);
                    keypress_mute_until = Some(std::time::Instant::now() + keypress_mute_duration);
                }

                // Unmute mic after keypress mute timeout
                if let Some(until) = keypress_mute_until {
                    if std::time::Instant::now() >= until {
                        mic_muted.store(false, Ordering::SeqCst);
                        runtime_state.mic_muted.store(false, Ordering::SeqCst);
                        keypress_mute_until = None;
                    }
                }

                // Cancel auto-submit timer on keyboard input only
                // (Voice input timer is managed by Final event handler)
                if ui_renderer.has_input_activity() {
                    auto_submit_deadline = None;
                }

                // Update auto-submit progress bar
                if let Some(deadline) = auto_submit_deadline {
                    let now = tokio::time::Instant::now();
                    if now >= deadline {
                        auto_submit_deadline = None;
                        ui_renderer.set_auto_submit_progress(None);
                        if let Some(line) = ui_renderer.take_input() {
                            if !line.is_empty() {
                                // Cancel any in-progress response
                                let _ = session_tx.send(session::SessionCommand::Cancel);
                                ui.show_final(&line);
                                while let Ok(event) = async_ui_rx.try_recv() {
                                    ui_renderer.handle_ui_event(event)?;
                                }
                                ui_renderer.draw()?;
                                let _ = session_tx.send(session::SessionCommand::UserInput(line));
                            }
                        }
                    } else {
                        let elapsed = auto_submit_delay.as_millis() as f32 - (deadline - now).as_millis() as f32;
                        let total = auto_submit_delay.as_millis() as f32;
                        ui_renderer.set_auto_submit_progress(Some(elapsed / total));
                    }
                } else {
                    ui_renderer.set_auto_submit_progress(None);
                }

                // Redraw
                ui_renderer.draw()?;
            }
        }
    }

    // Final cleanup before dropping UI
    ui_renderer.cleanup()?;
    drop(ui_renderer);
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
    TtsLevel(f32),
}

async fn run_probe(prompt: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::load();
    let system_prompt = chat::system_prompt(&config.name);

    let mut backend: Box<dyn llm::LlmBackend> = match &config.llm {
        #[cfg(feature = "openai-compat")]
        LlmConfig::OpenAiCompat {
            base_url,
            model,
            api_key,
            temperature,
            top_p,
            max_tokens,
            presence_penalty,
            frequency_penalty,
            ..
        } => Box::new(llm::openai_compat::OpenAiCompatBackend::new(
            base_url.clone(),
            model.clone(),
            api_key.clone(),
            *temperature,
            *top_p,
            *max_tokens,
            *presence_penalty,
            *frequency_penalty,
        )?),
        #[cfg(feature = "ollama")]
        LlmConfig::Ollama { model } => {
            Box::new(llm::ollama::OllamaBackend::new(model, &system_prompt))
        }
        _ => {
            eprintln!("Probe requires openai-compat or ollama backend");
            return Ok(());
        }
    };

    let messages = vec![
        llm::Message { role: llm::Role::System, content: system_prompt },
        llm::Message { role: llm::Role::User, content: prompt.to_string() },
    ];

    print!("\x1b[36m"); // cyan
    let result = backend.generate(&messages, &mut |token| {
        print!("{}", token);
        use std::io::Write;
        std::io::stdout().flush().ok();
    });
    println!("\x1b[0m"); // reset

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }
    Ok(())
}

async fn run_transcribe_mode() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
    let (final_tx, final_rx) = mpsc::channel::<Arc<[f32]>>();
    let (preview_tx, _) = mpsc::sync_channel::<Arc<[f32]>>(1); // unused but required
    let (display_tx, display_rx) = mpsc::channel::<DisplayEvent>();

    let _stream = audio::start_capture(audio_tx)?;

    let tts_playing = Arc::new(AtomicBool::new(false));
    let tts_playing_vad = Arc::clone(&tts_playing);
    let mic_muted = Arc::new(AtomicBool::new(false));
    let mic_muted_vad = Arc::clone(&mic_muted);

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
            mic_muted_vad,
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
