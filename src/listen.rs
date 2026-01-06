use crate::transcriber::Transcriber;
use crate::vad::VadEngine;
use screencapturekit::prelude::*;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

const TARGET_RATE: usize = 16000;
const VAD_FRAME_SAMPLES: usize = 480;

fn print_status(status: &str, last: &mut String) {
    if status != last {
        print!("\r\x1b[K{}", status);
        std::io::stdout().flush().ok();
        *last = status.to_string();
    }
}

fn clear_status() {
    print!("\r\x1b[K");
    std::io::stdout().flush().ok();
}
const VAD_MODEL_PATH: &str = "models/silero_vad_v4.onnx";
const PARAKEET_MODEL_PATH: &str = "models/parakeet-tdt-0.6b-v3-int8";
const CAPTURE_SAMPLE_RATE: usize = 48000;
const OGG_BITRATE: i32 = 64000;

#[derive(Debug, Clone)]
pub enum AudioSource {
    Mic,
    System,
    App(String),
}

fn resample(samples: &[f32], from_rate: usize, to_rate: usize) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let new_len = (samples.len() as f64 * ratio) as usize;
    (0..new_len)
        .map(|i| {
            let src_idx = i as f64 / ratio;
            let idx = src_idx as usize;
            let frac = src_idx - idx as f64;
            if idx + 1 < samples.len() {
                samples[idx] * (1.0 - frac as f32) + samples[idx + 1] * frac as f32
            } else {
                samples.get(idx).copied().unwrap_or(0.0)
            }
        })
        .collect()
}

pub fn list_apps() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let content = SCShareableContent::get()?;
    println!("Running applications:\n");
    for app in content.applications() {
        println!("  {} ({})", app.application_name(), app.bundle_identifier());
    }
    Ok(())
}

pub fn pick_source_interactive() -> Result<AudioSource, Box<dyn std::error::Error + Send + Sync>> {
    let content = SCShareableContent::get()?;
    let apps: Vec<_> = content.applications().into_iter().collect();

    println!("\nSelect audio source:\n");
    println!("  [0] System microphone");
    println!("  [1] System audio (all apps)");
    println!("\nOr pick an application:");
    for (i, app) in apps.iter().enumerate() {
        println!("  [{}] {}", i + 2, app.application_name());
    }

    print!("\nChoice: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let choice: usize = input.trim().parse().unwrap_or(0);

    Ok(match choice {
        0 => AudioSource::Mic,
        1 => AudioSource::System,
        n if n >= 2 && n - 2 < apps.len() => {
            AudioSource::App(apps[n - 2].application_name().to_string())
        }
        _ => AudioSource::Mic,
    })
}

pub fn run_listen(
    source: AudioSource,
    output: PathBuf,
    debug_wav: Option<PathBuf>,
    save_ogg: Option<PathBuf>,
    no_vad: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;

    let file = File::create(&output)?;
    let mut writer = BufWriter::new(file);

    let debug_samples: Arc<std::sync::Mutex<Vec<f32>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let debug_samples_clone = debug_samples.clone();
    let save_debug = debug_wav.is_some() || save_ogg.is_some();

    println!("Loading transcription model...");
    let mut transcriber = Transcriber::new(PARAKEET_MODEL_PATH)?;
    let mut vad = if no_vad {
        println!("VAD disabled - using fixed 3s chunks");
        None
    } else {
        Some(
            VadEngine::silero(VAD_MODEL_PATH, TARGET_RATE)
                .map_err(|e| format!("Failed to load VAD: {}", e))?,
        )
    };

    let (tx, rx): (Sender<Vec<f32>>, Receiver<Vec<f32>>) = mpsc::channel();

    match &source {
        AudioSource::Mic => run_mic_capture(
            tx,
            rx,
            &mut transcriber,
            &mut vad,
            &mut writer,
            running,
            debug_samples_clone,
            save_debug,
        )?,
        AudioSource::System | AudioSource::App(_) => run_screen_capture(
            source,
            tx,
            rx,
            &mut transcriber,
            &mut vad,
            &mut writer,
            running,
            debug_samples_clone,
            save_debug,
        )?,
    }

    writer.flush()?;
    println!("\nTranscription saved to: {}", output.display());

    let samples = debug_samples.lock().unwrap();
    let duration_secs = samples.len() as f32 / TARGET_RATE as f32;

    if let Some(wav_path) = debug_wav {
        save_wav(&wav_path, &samples, TARGET_RATE as u32)?;
        let size = std::fs::metadata(&wav_path)?.len();
        println!(
            "WAV saved: {} ({:.1}s, {:.1} KB)",
            wav_path.display(),
            duration_secs,
            size as f64 / 1024.0
        );
    }

    if let Some(ogg_path) = save_ogg {
        save_ogg_file(&ogg_path, &samples, TARGET_RATE as u32)?;
        let size = std::fs::metadata(&ogg_path)?.len();
        println!(
            "OGG saved: {} ({:.1}s, {:.1} KB, {}kbps)",
            ogg_path.display(),
            duration_secs,
            size as f64 / 1024.0,
            OGG_BITRATE / 1000
        );
    }

    Ok(())
}

fn save_wav(
    path: &PathBuf,
    samples: &[f32],
    sample_rate: u32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut file = File::create(path)?;
    let num_samples = samples.len() as u32;
    let byte_rate = sample_rate * 2;
    let data_size = num_samples * 2;

    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size).to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;

    for &s in samples {
        let i = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
        file.write_all(&i.to_le_bytes())?;
    }
    Ok(())
}

fn save_ogg_file(
    path: &PathBuf,
    samples: &[f32],
    sample_rate: u32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::num::NonZero;
    use vorbis_rs::VorbisEncoderBuilder;

    let file = File::create(path)?;
    let mut encoder = VorbisEncoderBuilder::new(
        NonZero::new(sample_rate).unwrap(),
        NonZero::new(1).unwrap(), // mono
        file,
    )?
    .build()?;

    // Encode in chunks
    const CHUNK_SIZE: usize = 4096;
    for chunk in samples.chunks(CHUNK_SIZE) {
        encoder.encode_audio_block([chunk])?;
    }

    encoder.finish()?;
    Ok(())
}

fn load_ogg_file(
    path: &PathBuf,
) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error + Send + Sync>> {
    use lewton::inside_ogg::OggStreamReader;

    let file = File::open(path)?;
    let mut reader = OggStreamReader::new(file)?;

    let sample_rate = reader.ident_hdr.audio_sample_rate;
    let channels = reader.ident_hdr.audio_channels as usize;

    let mut samples = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl()? {
        // Convert i16 to f32 and mix to mono if needed
        for chunk in packet.chunks(channels) {
            let mono: f32 =
                chunk.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32;
            samples.push(mono);
        }
    }

    Ok((samples, sample_rate))
}

fn run_mic_capture(
    tx: Sender<Vec<f32>>,
    rx: Receiver<Vec<f32>>,
    transcriber: &mut Transcriber,
    vad: &mut Option<VadEngine>,
    writer: &mut BufWriter<File>,
    running: Arc<AtomicBool>,
    debug_samples: Arc<std::sync::Mutex<Vec<f32>>>,
    save_debug: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No input device")?;
    let supported = device.default_input_config()?;
    let sample_rate = u32::from(supported.sample_rate()) as usize;
    let channels = supported.channels() as usize;

    println!(
        "Recording from microphone ({}Hz {}ch)...",
        sample_rate, channels
    );
    println!("Press Ctrl+C to stop.\n");

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
            let _ = tx.send(resampled);
        },
        |e| eprintln!("Stream error: {}", e),
        None,
    )?;
    stream.play()?;

    process_audio(
        rx,
        transcriber,
        vad,
        writer,
        running,
        debug_samples,
        save_debug,
    )?;
    drop(stream);
    Ok(())
}

fn run_screen_capture(
    source: AudioSource,
    tx: Sender<Vec<f32>>,
    rx: Receiver<Vec<f32>>,
    transcriber: &mut Transcriber,
    vad: &mut Option<VadEngine>,
    writer: &mut BufWriter<File>,
    running: Arc<AtomicBool>,
    debug_samples: Arc<std::sync::Mutex<Vec<f32>>>,
    save_debug: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let content = SCShareableContent::get()?;
    let display = content.displays().into_iter().next().ok_or("No display")?;

    let filter = match &source {
        AudioSource::System => SCContentFilter::create()
            .with_display(&display)
            .with_excluding_windows(&[])
            .build(),
        AudioSource::App(name) => {
            let name_lower = name.to_lowercase();
            let app = content
                .applications()
                .into_iter()
                .find(|a| a.application_name().to_lowercase().contains(&name_lower))
                .ok_or_else(|| format!("App '{}' not found", name))?;
            println!("Capturing from: {}", app.application_name());
            SCContentFilter::create()
                .with_display(&display)
                .with_including_applications(&[&app], &[])
                .build()
        }
        AudioSource::Mic => unreachable!(),
    };

    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_sample_rate(CAPTURE_SAMPLE_RATE as i32)
        .with_channel_count(1);

    let mut stream = SCStream::new(&filter, &config);

    let buffer_count = Arc::new(AtomicUsize::new(0));
    let buffer_count_clone = buffer_count.clone();
    let sample_count = Arc::new(AtomicUsize::new(0));
    let sample_count_clone = sample_count.clone();

    stream.add_output_handler(
        move |sample: CMSampleBuffer, of_type: SCStreamOutputType| {
            if !matches!(of_type, SCStreamOutputType::Audio) {
                return;
            }

            buffer_count_clone.fetch_add(1, Ordering::Relaxed);

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

                    sample_count_clone.fetch_add(samples.len(), Ordering::Relaxed);

                    let resampled = resample(&samples, CAPTURE_SAMPLE_RATE, TARGET_RATE);
                    let _ = tx.send(resampled);
                }
            }
        },
        SCStreamOutputType::Audio,
    );

    println!("Recording... Press Ctrl+C to stop.\n");
    stream.start_capture()?;

    process_audio(
        rx,
        transcriber,
        vad,
        writer,
        running,
        debug_samples,
        save_debug,
    )?;

    stream.stop_capture()?;

    println!(
        "Audio buffers received: {}",
        buffer_count.load(Ordering::Relaxed)
    );
    println!(
        "Total samples received: {}",
        sample_count.load(Ordering::Relaxed)
    );

    Ok(())
}

fn format_timestamp(secs: f32) -> String {
    let mins = (secs / 60.0) as u32;
    let secs = secs % 60.0;
    format!("{:02}:{:05.2}", mins, secs)
}

struct Paragraph {
    start: f32,
    end: f32,
    text: String,
}

struct ContinuousTranscriber {
    committed_words: Vec<(f32, f32, String)>, // (start, end, word)
    overlap_audio: Vec<f32>,
    overlap_duration: f32,
    time_offset: f32,
    paragraphs: Vec<Paragraph>,               // Recent paragraphs for repaint
    lines_printed: usize,                     // How many lines to clear on repaint
}

impl ContinuousTranscriber {
    fn new() -> Self {
        Self {
            committed_words: Vec::new(),
            overlap_audio: Vec::new(),
            overlap_duration: 1.0,
            time_offset: 0.0,
            paragraphs: Vec::new(),
            lines_printed: 0,
        }
    }

    fn repaint(&self) {
        // Move up and clear lines
        for _ in 0..self.lines_printed {
            print!("\x1b[A\x1b[K");
        }
        // Reprint paragraphs
        for p in &self.paragraphs {
            println!("[{}-{}] {}", format_timestamp(p.start), format_timestamp(p.end), p.text);
        }
        std::io::stdout().flush().ok();
    }

    fn process_chunk(
        &mut self,
        audio: &[f32],
        transcriber: &mut crate::transcriber::Transcriber,
    ) -> Option<(String, bool)> { // Returns (output, needs_repaint)
        let mut full_audio = self.overlap_audio.clone();
        full_audio.extend_from_slice(audio);
        
        let chunk_duration = full_audio.len() as f32 / TARGET_RATE as f32;
        let overlap_secs = self.overlap_audio.len() as f32 / TARGET_RATE as f32;
        
        let text = match transcriber.transcribe(&full_audio) {
            Ok(t) => t,
            Err(_) => return None,
        };
        
        let text = text.trim();
        if text.is_empty() {
            self.time_offset += audio.len() as f32 / TARGET_RATE as f32;
            return None;
        }

        // Use full text, not segments (segments are subword tokens)
        let start = self.time_offset;
        let end = self.time_offset + chunk_duration - overlap_secs;

        // Split into words from the full text
        let new_words: Vec<(f32, f32, String)> = text
            .split_whitespace()
            .map(|w| (start, end, w.to_string()))
            .collect();

        // Find alignment with committed words
        let correction_window = 5.min(self.committed_words.len());
        let mut match_idx = 0;
        let mut correction_needed = false;
        
        if correction_window > 0 {
            let committed_tail: Vec<&str> = self.committed_words[self.committed_words.len() - correction_window..]
                .iter()
                .map(|(_, _, w)| w.as_str())
                .collect();
            
            for i in 0..new_words.len().saturating_sub(correction_window - 1) {
                let check_len = correction_window.min(new_words.len() - i);
                let new_slice: Vec<&str> = new_words[i..i + check_len]
                    .iter()
                    .map(|(_, _, w)| w.as_str())
                    .collect();
                
                // Check for exact match
                if new_slice == committed_tail[..check_len] {
                    match_idx = i + check_len;
                    break;
                }
                
                // Check for partial match with correction
                if i == 0 && check_len >= 2 {
                    let matches = new_slice.iter().zip(committed_tail.iter())
                        .filter(|(a, b)| a == b)
                        .count();
                    if matches >= check_len - 2 { // Allow up to 2 word corrections
                        // Correction detected - update committed words
                        let correction_start = self.committed_words.len() - correction_window;
                        for (j, (start, end, word)) in new_words[..check_len].iter().enumerate() {
                            if correction_start + j < self.committed_words.len() {
                                self.committed_words[correction_start + j] = (*start, *end, word.clone());
                            }
                        }
                        correction_needed = true;
                        match_idx = check_len;
                        break;
                    }
                }
            }
        }

        // Rebuild paragraphs if correction needed
        if correction_needed {
            self.rebuild_paragraphs();
        }

        // Output new words
        let output_words = &new_words[match_idx..];
        
        let result = if !output_words.is_empty() {
            let start_time = output_words.first().map(|(s, _, _)| *s).unwrap_or(self.time_offset);
            let end_time = output_words.last().map(|(_, e, _)| *e).unwrap_or(start_time);
            let text: String = output_words.iter().map(|(_, _, w)| w.as_str()).collect::<Vec<_>>().join(" ");
            
            self.committed_words.extend(output_words.iter().cloned());
            
            // Add to paragraphs
            self.paragraphs.push(Paragraph { start: start_time, end: end_time, text: text.clone() });
            // Keep only last 10 paragraphs for repaint
            if self.paragraphs.len() > 10 {
                self.lines_printed = self.lines_printed.saturating_sub(1);
                self.paragraphs.remove(0);
            }
            
            Some((format!("[{}-{}] {}", format_timestamp(start_time), format_timestamp(end_time), text), correction_needed))
        } else {
            if correction_needed {
                Some((String::new(), true)) // Signal repaint only
            } else {
                None
            }
        };

        // Keep overlap
        let overlap_samples = (self.overlap_duration * TARGET_RATE as f32) as usize;
        if audio.len() > overlap_samples {
            self.overlap_audio = audio[audio.len() - overlap_samples..].to_vec();
        } else {
            self.overlap_audio = audio.to_vec();
        }
        
        self.time_offset += (audio.len() as f32 / TARGET_RATE as f32) - self.overlap_duration;
        self.time_offset = self.time_offset.max(0.0);

        result
    }

    fn rebuild_paragraphs(&mut self) {
        // Rebuild paragraph text from committed words
        // Group words by their paragraph based on time proximity
        self.paragraphs.clear();
        
        if self.committed_words.is_empty() {
            return;
        }

        let mut current_start = self.committed_words[0].0;
        let mut current_end = self.committed_words[0].1;
        let mut current_words: Vec<&str> = vec![&self.committed_words[0].2];
        
        for (start, end, word) in &self.committed_words[1..] {
            if *start - current_end > 2.0 { // Gap > 2s = new paragraph
                self.paragraphs.push(Paragraph {
                    start: current_start,
                    end: current_end,
                    text: current_words.join(" "),
                });
                current_start = *start;
                current_words.clear();
            }
            current_end = *end;
            current_words.push(word);
        }
        
        if !current_words.is_empty() {
            self.paragraphs.push(Paragraph {
                start: current_start,
                end: current_end,
                text: current_words.join(" "),
            });
        }

        // Keep only last 10
        while self.paragraphs.len() > 10 {
            self.paragraphs.remove(0);
        }
    }

    fn flush(&mut self, audio: &[f32], transcriber: &mut crate::transcriber::Transcriber) -> Option<String> {
        if audio.is_empty() && self.overlap_audio.is_empty() {
            return None;
        }
        
        let mut full_audio = self.overlap_audio.clone();
        full_audio.extend_from_slice(audio);
        
        if full_audio.len() < TARGET_RATE / 2 {
            return None;
        }

        let text = match transcriber.transcribe(&full_audio) {
            Ok(t) => t,
            Err(_) => return None,
        };

        let committed_text: String = self.committed_words.iter()
            .map(|(_, _, w)| w.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        
        let new_text = if text.starts_with(&committed_text) {
            text[committed_text.len()..].trim()
        } else {
            text.trim()
        };

        if new_text.is_empty() {
            return None;
        }

        let end_time = self.time_offset + full_audio.len() as f32 / TARGET_RATE as f32;
        Some(format!("[{}-{}] {}", format_timestamp(self.time_offset), format_timestamp(end_time), new_text))
    }
}

fn process_audio(
    rx: Receiver<Vec<f32>>,
    transcriber: &mut Transcriber,
    vad: &mut Option<VadEngine>,
    writer: &mut BufWriter<File>,
    running: Arc<AtomicBool>,
    debug_samples: Arc<std::sync::Mutex<Vec<f32>>>,
    save_debug: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut speech_buf: Vec<f32> = Vec::new();
    let mut silence_frames = 0;
    let mut in_speech = false;
    let mut last_status = String::new();
    
    let mut cont = ContinuousTranscriber::new();
    let mut chunk_buf: Vec<f32> = Vec::new();
    let chunk_size = TARGET_RATE * 3;

    while running.load(Ordering::SeqCst) {
        let samples = match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(s) => s,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                print_status("⏳ waiting for audio...", &mut last_status);
                continue;
            }
            Err(_) => break,
        };

        if save_debug {
            debug_samples.lock().unwrap().extend_from_slice(&samples);
        }

        if let Some(vad_engine) = vad {
            for chunk in samples.chunks(VAD_FRAME_SAMPLES) {
                if chunk.len() < VAD_FRAME_SAMPLES {
                    continue;
                }
                let is_speech = vad_engine.is_speech(chunk, in_speech);

                if is_speech {
                    silence_frames = 0;
                    in_speech = true;
                    speech_buf.extend_from_slice(chunk);
                    let secs = speech_buf.len() as f32 / TARGET_RATE as f32;
                    print_status(&format!("🎤 speech [{:.1}s]", secs), &mut last_status);
                } else if in_speech {
                    silence_frames += 1;
                    speech_buf.extend_from_slice(chunk);
                    print_status(&format!("🔇 silence ({})", silence_frames), &mut last_status);

                    if silence_frames >= 15 {
                        if speech_buf.len() >= TARGET_RATE / 2 {
                            print_status("⚙️  transcribing...", &mut last_status);
                            match transcriber.transcribe(&speech_buf) {
                                Ok(text) if !text.is_empty() => {
                                    clear_status();
                                    println!("> {}", text);
                                    writeln!(writer, "{}", text)?;
                                    writer.flush()?;
                                }
                                Ok(_) => {}
                                Err(e) => eprintln!("Transcription error: {}", e),
                            }
                        }
                        speech_buf.clear();
                        in_speech = false;
                        silence_frames = 0;
                    }
                } else {
                    print_status("👂 listening...", &mut last_status);
                }
            }
        } else {
            chunk_buf.extend_from_slice(&samples);
            let secs = chunk_buf.len() as f32 / TARGET_RATE as f32;
            print_status(&format!("📥 [{:.1}s]", secs), &mut last_status);

            if chunk_buf.len() >= chunk_size {
                print_status("⚙️  transcribing...", &mut last_status);
                if let Some((output, needs_repaint)) = cont.process_chunk(&chunk_buf, transcriber) {
                    clear_status();
                    if needs_repaint {
                        cont.repaint();
                    } else if !output.is_empty() {
                        println!("{}", output);
                        cont.lines_printed += 1;
                    }
                    // Write latest to file (without repaint logic)
                    if !output.is_empty() {
                        writeln!(writer, "{}", output)?;
                        writer.flush()?;
                    }
                }
                chunk_buf.clear();
            }
        }
    }

    if let Some(output) = cont.flush(&chunk_buf, transcriber) {
        clear_status();
        println!("{}", output);
        writeln!(writer, "{}", output)?;
    }
    Ok(())
}

pub fn transcribe_wav(path: PathBuf) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let (samples, sample_rate) = if ext == "ogg" {
        println!("Loading OGG: {:?}", path);
        load_ogg_file(&path)?
    } else {
        println!("Loading WAV: {:?}", path);
        load_wav_file(&path)?
    };

    println!(
        "Sample rate: {}Hz, {} samples ({:.1}s)",
        sample_rate,
        samples.len(),
        samples.len() as f32 / sample_rate as f32
    );

    let samples = if sample_rate as usize != TARGET_RATE {
        println!("Resampling {}Hz -> {}Hz", sample_rate, TARGET_RATE);
        resample(&samples, sample_rate as usize, TARGET_RATE)
    } else {
        samples
    };

    println!("Loading transcription model...");
    let mut transcriber = Transcriber::new(PARAKEET_MODEL_PATH)?;

    println!("Transcribing...\n");
    let start = std::time::Instant::now();
    let text = transcriber.transcribe(&samples)?;
    let elapsed = start.elapsed();

    println!("{}", text);
    println!("\n---");
    println!(
        "Audio: {:.1}s | Transcribed in {:.1}s ({:.1}x realtime)",
        samples.len() as f32 / TARGET_RATE as f32,
        elapsed.as_secs_f32(),
        (samples.len() as f32 / TARGET_RATE as f32) / elapsed.as_secs_f32()
    );

    Ok(())
}

fn load_wav_file(
    path: &PathBuf,
) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error + Send + Sync>> {
    let mut file = File::open(path)?;

    let mut header = [0u8; 44];
    file.read_exact(&mut header)?;

    let sample_rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
    let bits_per_sample = u16::from_le_bytes([header[34], header[35]]);
    let data_size = u32::from_le_bytes([header[40], header[41], header[42], header[43]]);

    let mut data = vec![0u8; data_size as usize];
    file.read_exact(&mut data)?;

    let samples: Vec<f32> = if bits_per_sample == 16 {
        data.chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect()
    } else if bits_per_sample == 32 {
        data.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    } else {
        return Err(format!("Unsupported bits per sample: {}", bits_per_sample).into());
    };

    Ok((samples, sample_rate))
}
