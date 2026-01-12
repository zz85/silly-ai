pub use crate::pipeline::{run_pipeline_with_options, AudioSource, run_multi_source};
use crate::capture::{resample, TARGET_RATE};
use crate::transcriber::Transcriber;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

const PARAKEET_MODEL_PATH: &str = "models/parakeet-tdt-0.6b-v3-int8";

pub fn list_apps() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let apps = crate::capture::list_apps()?;
    println!("Running applications:\n");
    for app in apps {
        println!("  {}", app);
    }
    Ok(())
}

pub fn pick_source_interactive() -> Result<AudioSource, Box<dyn std::error::Error + Send + Sync>> {
    pick_source_with_apps(&crate::capture::list_apps()?)
}

fn pick_source_with_apps(apps: &[String]) -> Result<AudioSource, Box<dyn std::error::Error + Send + Sync>> {
    println!("\nSelect audio source:\n");
    println!("  [0] System microphone");
    println!("  [1] System audio (all apps)");
    println!("\nOr pick an application:");
    for (i, app) in apps.iter().enumerate() {
        println!("  [{}] {}", i + 2, app);
    }

    print!("\nChoice: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let choice: usize = input.trim().parse().unwrap_or(0);

    Ok(match choice {
        0 => AudioSource::Mic,
        1 => AudioSource::System,
        n if n >= 2 && n - 2 < apps.len() => AudioSource::App(apps[n - 2].clone()),
        _ => AudioSource::Mic,
    })
}

pub fn pick_sources_multi() -> Result<(AudioSource, AudioSource), Box<dyn std::error::Error + Send + Sync>> {
    let apps = crate::capture::list_apps()?;

    println!("\nSelect TWO audio sources for multi-source transcription.\n");
    println!("  [0] System microphone");
    println!("  [1] System audio (all apps)");
    println!("\nOr pick an application:");
    for (i, app) in apps.iter().enumerate() {
        println!("  [{}] {}", i + 2, app);
    }

    let parse_choice = |choice: usize| -> AudioSource {
        match choice {
            0 => AudioSource::Mic,
            1 => AudioSource::System,
            n if n >= 2 && n - 2 < apps.len() => AudioSource::App(apps[n - 2].clone()),
            _ => AudioSource::Mic,
        }
    };

    print!("\nFirst source (e.g. mic) [0]: ");
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let choice1: usize = input.trim().parse().unwrap_or(0);
    let source1 = parse_choice(choice1);

    print!("Second source (e.g. app) [1]: ");
    std::io::stdout().flush()?;
    input.clear();
    std::io::stdin().read_line(&mut input)?;
    let choice2: usize = input.trim().parse().unwrap_or(1);
    let source2 = parse_choice(choice2);

    Ok((source1, source2))
}

pub fn run_listen(
    source: AudioSource,
    output: PathBuf,
    _debug_wav: Option<PathBuf>,
    save_ogg: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    run_pipeline_with_options(source, output, save_ogg)
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

fn load_wav_file(path: &PathBuf) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error + Send + Sync>> {
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

fn load_ogg_file(path: &PathBuf) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error + Send + Sync>> {
    use lewton::inside_ogg::OggStreamReader;

    let file = File::open(path)?;
    let mut reader = OggStreamReader::new(file)?;

    let sample_rate = reader.ident_hdr.audio_sample_rate;
    let channels = reader.ident_hdr.audio_channels as usize;

    let mut samples = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl()? {
        for chunk in packet.chunks(channels) {
            let mono: f32 = chunk.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32;
            samples.push(mono);
        }
    }

    Ok((samples, sample_rate))
}
