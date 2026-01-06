use crate::capture::{capture_mic, capture_system, TARGET_RATE};
use crate::segmenter::{run_segmenter, AudioSegment, SegmenterConfig};
use crate::transcriber::Transcriber;
use crate::vad::VadEngine;
use flume::{Receiver, Sender};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

const VAD_MODEL_PATH: &str = "models/silero_vad_v4.onnx";
const PARAKEET_MODEL_PATH: &str = "models/parakeet-tdt-0.6b-v3-int8";

#[derive(Clone, Debug)]
pub struct Transcript {
    pub start: f32,
    pub end: f32,
    pub text: String,
}

#[derive(Debug, Clone)]
pub enum AudioSource {
    Mic,
    System,
    App(String),
}

pub fn run_transcriber(
    rx: Receiver<AudioSegment>,
    tx: Sender<Transcript>,
    transcriber: Transcriber,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut transcriber = transcriber;

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(segment) => {
                if let Ok(text) = transcriber.transcribe(&segment.samples) {
                    let text = text.trim();
                    if !text.is_empty() {
                        let _ = tx.send(Transcript {
                            start: segment.start_secs(),
                            end: segment.start_secs() + segment.duration_secs(),
                            text: text.to_string(),
                        });
                    }
                }
            }
            Err(flume::RecvTimeoutError::Timeout) => continue,
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Drain remaining
    for segment in rx.drain() {
        if let Ok(text) = transcriber.transcribe(&segment.samples) {
            let text = text.trim();
            if !text.is_empty() {
                let _ = tx.send(Transcript {
                    start: segment.start_secs(),
                    end: segment.start_secs() + segment.duration_secs(),
                    text: text.to_string(),
                });
            }
        }
    }

    Ok(())
}

pub fn run_writer(
    rx: Receiver<Transcript>,
    output: PathBuf,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = File::create(&output)?;
    let mut writer = BufWriter::new(file);

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(t) => {
                let line = format!("[{:.2}-{:.2}] {}", t.start, t.end, t.text);
                println!("{}", line);
                writeln!(writer, "{}", line)?;
                writer.flush()?;
            }
            Err(flume::RecvTimeoutError::Timeout) => continue,
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Drain remaining
    for t in rx.drain() {
        let line = format!("[{:.2}-{:.2}] {}", t.start, t.end, t.text);
        println!("{}", line);
        writeln!(writer, "{}", line)?;
    }

    writer.flush()?;
    println!("\nSaved to: {}", output.display());
    Ok(())
}

pub fn run_pipeline(
    source: AudioSource,
    output: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;

    // Load models first (before spawning threads)
    println!("Loading VAD...");
    let vad = VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE)?;

    println!("Loading transcriber...");
    let transcriber = Transcriber::new(PARAKEET_MODEL_PATH)?;

    // Channels
    let (audio_tx, audio_rx) = flume::bounded::<Vec<f32>>(100);
    let (segment_tx, segment_rx) = flume::bounded::<AudioSegment>(10);
    let (transcript_tx, transcript_rx) = flume::bounded::<Transcript>(10);

    // Spawn threads
    let running_capture = running.clone();
    let capture_handle = thread::spawn(move || {
        let result = match source {
            AudioSource::Mic => capture_mic(audio_tx, running_capture),
            AudioSource::System => capture_system(audio_tx, running_capture, None),
            AudioSource::App(name) => capture_system(audio_tx, running_capture, Some(name)),
        };
        if let Err(e) = result {
            eprintln!("Capture error: {}", e);
        }
    });

    let running_seg = running.clone();
    let segmenter_handle = thread::spawn(move || {
        if let Err(e) = run_segmenter(
            audio_rx,
            segment_tx,
            vad,
            SegmenterConfig::default(),
            running_seg,
        ) {
            eprintln!("Segmenter error: {}", e);
        }
    });

    let running_trans = running.clone();
    let transcriber_handle = thread::spawn(move || {
        if let Err(e) = run_transcriber(segment_rx, transcript_tx, transcriber, running_trans) {
            eprintln!("Transcriber error: {}", e);
        }
    });

    // Writer runs on main thread
    println!("Recording... Press Ctrl+C to stop.\n");
    run_writer(transcript_rx, output, running.clone())?;

    // Wait for threads
    let _ = capture_handle.join();
    let _ = segmenter_handle.join();
    let _ = transcriber_handle.join();

    Ok(())
}
