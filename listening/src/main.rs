use clap::{Parser, Subcommand};
use screencapturekit::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "listening")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all running applications
    List,
    /// Capture audio from an application
    Capture {
        /// Application name (or part of it)
        name: String,
    },
}

struct AudioHandler {
    sample_count: AtomicUsize,
}

impl SCStreamOutputTrait for AudioHandler {
    fn did_output_sample_buffer(&self, _sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if matches!(of_type, SCStreamOutputType::Audio) {
            let count = self.sample_count.fetch_add(1, Ordering::Relaxed);
            if count % 50 == 0 {
                println!("🔊 Audio samples received: {}", count);
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let content = SCShareableContent::get()?;

    match cli.command {
        Commands::List => {
            println!("Running applications:\n");
            for app in content.applications() {
                println!("  {} ({})", app.application_name(), app.bundle_identifier());
            }
        }
        Commands::Capture { name } => {
            let name_lower = name.to_lowercase();
            let app = content
                .applications()
                .into_iter()
                .find(|a| a.application_name().to_lowercase().contains(&name_lower))
                .ok_or_else(|| format!("App '{}' not found", name))?;

            println!("Capturing audio from: {}", app.application_name());

            let display = content.displays().into_iter().next().ok_or("No display")?;

            let filter = SCContentFilter::create()
                .with_display(&display)
                .with_including_applications(&[&app], &[])
                .build();

            let config = SCStreamConfiguration::new()
                .with_width(2)
                .with_height(2)
                .with_captures_audio(true)
                .with_sample_rate(48000)
                .with_channel_count(2);

            let mut stream = SCStream::new(&filter, &config);
            let handler = AudioHandler {
                sample_count: AtomicUsize::new(0),
            };
            stream.add_output_handler(handler, SCStreamOutputType::Audio);

            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();
            ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;

            stream.start_capture()?;
            println!("Recording... Press Ctrl+C to stop");

            while running.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            stream.stop_capture()?;
            println!("\nStopped.");
        }
    }
    Ok(())
}
