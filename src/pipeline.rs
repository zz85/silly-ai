use crate::capture::{capture_mic, capture_system, TARGET_RATE};
use crate::segmenter::{run_segmenter, AudioSegment, SegmenterConfig};
use crate::transcriber::Transcriber;
use crate::vad::VadEngine;
use flume::{Receiver, Sender};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::num::NonZero;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use vorbis_rs::VorbisEncoderBuilder;

const VAD_MODEL_PATH: &str = "models/silero_vad_v4.onnx";
const PARAKEET_MODEL_PATH: &str = "models/parakeet-tdt-0.6b-v3-int8";

#[derive(Clone, Debug)]
pub struct Transcript {
    pub start: f32,
    pub end: f32,
    pub text: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AudioSource {
    Mic,
    System,
    App(String),
}

impl AudioSource {
    pub fn label(&self) -> String {
        match self {
            AudioSource::Mic => "mic".to_string(),
            AudioSource::System => "system".to_string(),
            AudioSource::App(name) => name.clone(),
        }
    }
}

pub fn run_transcriber(
    rx: Receiver<AudioSegment>,
    tx: Sender<Transcript>,
    transcriber: Transcriber,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_transcriber_with_source(rx, tx, transcriber, running, None)
}

pub fn run_transcriber_with_source(
    rx: Receiver<AudioSegment>,
    tx: Sender<Transcript>,
    transcriber: Transcriber,
    running: Arc<AtomicBool>,
    source: Option<String>,
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
                            source: source.clone(),
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
                    source: source.clone(),
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

    let format_line = |t: &Transcript| -> String {
        match &t.source {
            Some(src) => format!("[{:.2}-{:.2}] [{}] {}", t.start, t.end, src, t.text),
            None => format!("[{:.2}-{:.2}] {}", t.start, t.end, t.text),
        }
    };

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(t) => {
                let line = format_line(&t);
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
        let line = format_line(&t);
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
    run_pipeline_with_options(source, output, None)
}

/// Record audio to OGG only, no transcription
pub fn run_record_only(
    source: AudioSource,
    ogg_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;

    let (ogg_tx, ogg_rx) = flume::bounded::<Vec<f32>>(100);

    let running_capture = running.clone();
    let capture_handle = thread::spawn(move || {
        let result = match source {
            AudioSource::Mic => capture_mic_with_tap(flume::bounded(1).0, Some(ogg_tx), running_capture),
            AudioSource::System => capture_system_with_tap(flume::bounded(1).0, Some(ogg_tx), running_capture, None),
            AudioSource::App(name) => capture_system_with_tap(flume::bounded(1).0, Some(ogg_tx), running_capture, Some(name)),
        };
        if let Err(e) = result {
            eprintln!("Capture error: {}", e);
        }
    });

    println!("Recording to {}... Press Ctrl+C to stop.\n", ogg_path.display());
    run_ogg_writer(ogg_rx, ogg_path, running)?;

    let _ = capture_handle.join();
    Ok(())
}

pub fn run_pipeline_with_options(
    source: AudioSource,
    output: PathBuf,
    save_ogg: Option<PathBuf>,
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

    // Optional: channel for OGG streaming
    let ogg_tx = save_ogg.as_ref().map(|_| {
        let (tx, rx) = flume::bounded::<Vec<f32>>(100);
        (tx, rx)
    });

    // Spawn threads
    let running_capture = running.clone();
    let ogg_sender = ogg_tx.as_ref().map(|(tx, _)| tx.clone());
    let capture_handle = thread::spawn(move || {
        let result = match source {
            AudioSource::Mic => capture_mic_with_tap(audio_tx, ogg_sender, running_capture),
            AudioSource::System => capture_system_with_tap(audio_tx, ogg_sender, running_capture, None),
            AudioSource::App(name) => capture_system_with_tap(audio_tx, ogg_sender, running_capture, Some(name)),
        };
        if let Err(e) = result {
            eprintln!("Capture error: {}", e);
        }
    });

    // OGG writer thread
    let ogg_handle = if let Some((_, ogg_rx)) = ogg_tx {
        let ogg_path = save_ogg.unwrap();
        let running_ogg = running.clone();
        Some(thread::spawn(move || {
            if let Err(e) = run_ogg_writer(ogg_rx, ogg_path, running_ogg) {
                eprintln!("OGG writer error: {}", e);
            }
        }))
    } else {
        None
    };

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
    if let Some(h) = ogg_handle {
        let _ = h.join();
    }

    Ok(())
}

fn run_ogg_writer(
    rx: Receiver<Vec<f32>>,
    path: PathBuf,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = File::create(&path)?;
    let mut encoder = VorbisEncoderBuilder::new(
        NonZero::new(TARGET_RATE as u32).unwrap(),
        NonZero::new(1).unwrap(),
        file,
    )?
    .build()?;

    let mut total_samples = 0usize;

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(samples) => {
                total_samples += samples.len();
                encoder.encode_audio_block([&samples[..]])?;
            }
            Err(flume::RecvTimeoutError::Timeout) => continue,
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Drain remaining
    for samples in rx.drain() {
        total_samples += samples.len();
        encoder.encode_audio_block([&samples[..]])?;
    }

    encoder.finish()?;
    
    let duration = total_samples as f32 / TARGET_RATE as f32;
    let size = std::fs::metadata(&path)?.len();
    println!("OGG saved: {} ({:.1}s, {:.1} KB)", path.display(), duration, size as f64 / 1024.0);
    
    Ok(())
}

fn capture_mic_with_tap(
    tx: Sender<Vec<f32>>,
    ogg_tx: Option<Sender<Vec<f32>>>,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use crate::capture::resample;

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No input device")?;
    let supported = device.default_input_config()?;
    let sample_rate = u32::from(supported.sample_rate()) as usize;
    let channels = supported.channels() as usize;

    println!("Mic: {}Hz {}ch", sample_rate, channels);

    let stream = device.build_input_stream(
        &supported.config(),
        move |data: &[f32], _| {
            let mono: Vec<f32> = if channels == 1 {
                data.to_vec()
            } else {
                data.chunks(channels)
                    .map(|c| c.iter().sum::<f32>() / channels as f32)
                    .collect()
            };
            let resampled = resample(&mono, sample_rate, TARGET_RATE);
            if let Some(ref ogg) = ogg_tx {
                let _ = ogg.send(resampled.clone());
            }
            let _ = tx.send(resampled);
        },
        |e| eprintln!("Mic error: {}", e),
        None,
    )?;
    stream.play()?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

fn capture_system_with_tap(
    tx: Sender<Vec<f32>>,
    ogg_tx: Option<Sender<Vec<f32>>>,
    running: Arc<AtomicBool>,
    app_filter: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use screencapturekit::prelude::*;
    use crate::capture::resample;

    const CAPTURE_SAMPLE_RATE: usize = 48000;

    let content = SCShareableContent::get()?;
    let display = content.displays().into_iter().next().ok_or("No display")?;

    let filter = if let Some(name) = &app_filter {
        let name_lower = name.to_lowercase();
        let app = content
            .applications()
            .into_iter()
            .find(|a| a.application_name().to_lowercase().contains(&name_lower))
            .ok_or_else(|| format!("App '{}' not found", name))?;
        println!("Capturing: {}", app.application_name());
        SCContentFilter::create()
            .with_display(&display)
            .with_including_applications(&[&app], &[])
            .build()
    } else {
        println!("Capturing: system audio");
        SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build()
    };

    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_sample_rate(CAPTURE_SAMPLE_RATE as i32)
        .with_channel_count(1);

    let mut stream = SCStream::new(&filter, &config);

    stream.add_output_handler(
        move |sample: CMSampleBuffer, of_type: SCStreamOutputType| {
            if !matches!(of_type, SCStreamOutputType::Audio) {
                return;
            }
            if let Some(audio_buffers) = sample.audio_buffer_list() {
                for buf in &audio_buffers {
                    let bytes = buf.data();
                    if bytes.is_empty() {
                        continue;
                    }
                    let samples: Vec<f32> = bytes
                        .chunks_exact(4)
                        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                        .collect();
                    let resampled = resample(&samples, CAPTURE_SAMPLE_RATE, TARGET_RATE);
                    if let Some(ref ogg) = ogg_tx {
                        let _ = ogg.send(resampled.clone());
                    }
                    let _ = tx.send(resampled);
                }
            }
        },
        SCStreamOutputType::Audio,
    );

    stream.start_capture()?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let _ = stream.stop_capture();
    Ok(())
}

/// Run two audio sources in parallel with merged, attributed transcripts
pub fn run_multi_source(
    source1: AudioSource,
    source2: AudioSource,
    output: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;

    // Load models (need 2 VADs, 2 transcribers)
    println!("Loading VAD models...");
    let vad1 = VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE)?;
    let vad2 = VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE)?;

    println!("Loading transcriber models...");
    let transcriber1 = Transcriber::new(PARAKEET_MODEL_PATH)?;
    let transcriber2 = Transcriber::new(PARAKEET_MODEL_PATH)?;

    // Shared transcript channel (both pipelines write here)
    let (transcript_tx, transcript_rx) = flume::bounded::<Transcript>(20);

    let label1 = source1.label();
    let label2 = source2.label();

    // Pipeline 1
    let (audio_tx1, audio_rx1) = flume::bounded::<Vec<f32>>(100);
    let (segment_tx1, segment_rx1) = flume::bounded::<AudioSegment>(10);
    let transcript_tx1 = transcript_tx.clone();

    let running1 = running.clone();
    let source1_clone = source1.clone();
    let capture1 = thread::spawn(move || {
        let result = match source1_clone {
            AudioSource::Mic => capture_mic_with_tap(audio_tx1, None, running1),
            AudioSource::System => capture_system_with_tap(audio_tx1, None, running1, None),
            AudioSource::App(name) => capture_system_with_tap(audio_tx1, None, running1, Some(name)),
        };
        if let Err(e) = result {
            eprintln!("Capture 1 error: {}", e);
        }
    });

    let running1_seg = running.clone();
    let seg1 = thread::spawn(move || {
        if let Err(e) = run_segmenter(audio_rx1, segment_tx1, vad1, SegmenterConfig::default(), running1_seg) {
            eprintln!("Segmenter 1 error: {}", e);
        }
    });

    let running1_trans = running.clone();
    let trans1 = thread::spawn(move || {
        if let Err(e) = run_transcriber_with_source(segment_rx1, transcript_tx1, transcriber1, running1_trans, Some(label1)) {
            eprintln!("Transcriber 1 error: {}", e);
        }
    });

    // Pipeline 2
    let (audio_tx2, audio_rx2) = flume::bounded::<Vec<f32>>(100);
    let (segment_tx2, segment_rx2) = flume::bounded::<AudioSegment>(10);

    let running2 = running.clone();
    let source2_clone = source2.clone();
    let capture2 = thread::spawn(move || {
        let result = match source2_clone {
            AudioSource::Mic => capture_mic_with_tap(audio_tx2, None, running2),
            AudioSource::System => capture_system_with_tap(audio_tx2, None, running2, None),
            AudioSource::App(name) => capture_system_with_tap(audio_tx2, None, running2, Some(name)),
        };
        if let Err(e) = result {
            eprintln!("Capture 2 error: {}", e);
        }
    });

    let running2_seg = running.clone();
    let seg2 = thread::spawn(move || {
        if let Err(e) = run_segmenter(audio_rx2, segment_tx2, vad2, SegmenterConfig::default(), running2_seg) {
            eprintln!("Segmenter 2 error: {}", e);
        }
    });

    let running2_trans = running.clone();
    let trans2 = thread::spawn(move || {
        if let Err(e) = run_transcriber_with_source(segment_rx2, transcript_tx, transcriber2, running2_trans, Some(label2)) {
            eprintln!("Transcriber 2 error: {}", e);
        }
    });

    // Writer on main thread
    println!("Recording from [{}] and [{}]... Press Ctrl+C to stop.\n", source1.label(), source2.label());
    run_writer(transcript_rx, output, running.clone())?;

    // Wait for threads
    let _ = capture1.join();
    let _ = capture2.join();
    let _ = seg1.join();
    let _ = seg2.join();
    let _ = trans1.join();
    let _ = trans2.join();

    Ok(())
}
