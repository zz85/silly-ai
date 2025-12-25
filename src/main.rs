mod audio;
mod transcriber;

use std::error::Error;
use std::io::Write;
use std::sync::mpsc;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (tx, rx) = mpsc::channel();

    let mut transcriber = transcriber::Transcriber::new("models/parakeet-tdt-0.6b-v3-int8")?;
    let _stream = audio::start_capture(tx)?;

    println!("Listening... Press Ctrl+C to stop.\n");

    let mut preview_text = String::new();

    for event in rx {
        match event {
            audio::AudioEvent::Preview(samples) => {
                if let Ok(text) = transcriber.transcribe(&samples) {
                    if !text.is_empty() && text != preview_text {
                        preview_text = text.clone();
                        print!("\r\x1b[K\x1b[90m{}\x1b[0m", text);
                        std::io::stdout().flush().ok();
                    }
                }
            }
            audio::AudioEvent::Final(samples) => {
                if let Ok(text) = transcriber.transcribe(&samples) {
                    if !text.is_empty() {
                        print!("\r\x1b[K{}\n", text);
                        std::io::stdout().flush().ok();
                    }
                }
                preview_text.clear();
            }
        }
    }

    Ok(())
}
