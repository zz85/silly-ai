use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};

const BARS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "▇"];
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

static FRAME: AtomicUsize = AtomicUsize::new(0);

pub fn audio_level(level: f32) -> String {
    let normalized = (level * 50.0).min(1.0);
    let count = (normalized * 5.0) as usize;
    if count == 0 {
        "     ".to_string()
    } else {
        BARS[1..=count.min(5)].join("") + &" ".repeat(5 - count.min(5))
    }
}

pub fn thinking() {
    print!("\r\x1b[K\x1b[33m⠋ Thinking...\x1b[0m");
    std::io::stdout().flush().ok();
}

pub fn speaking(frame: usize) {
    const FRAMES: [&str; 4] = ["♪", "♫", "♪", "♬"];
    print!("\r\x1b[K\x1b[35m{} Speaking...\x1b[0m", FRAMES[frame % FRAMES.len()]);
    std::io::stdout().flush().ok();
}

pub fn clear_line() {
    print!("\r\x1b[K");
    std::io::stdout().flush().ok();
}

pub fn show_level(level: f32) {
    let frame = FRAME.fetch_add(1, Ordering::Relaxed);
    let spinner = SPINNER[frame % SPINNER.len()];
    let bars = audio_level(level);
    print!("\r\x1b[K\x1b[90m{} {}\x1b[0m", spinner, bars);
    std::io::stdout().flush().ok();
}

pub fn show_preview(text: &str) {
    print!("\r\x1b[K\x1b[90m{}\x1b[0m", text);
    std::io::stdout().flush().ok();
}

pub fn show_final(text: &str) {
    print!("\r\x1b[K> {}\n", text);
    std::io::stdout().flush().ok();
}

pub fn start_response() {
    print!("\r\x1b[K\x1b[36m");
    std::io::stdout().flush().ok();
}

pub fn end_response() {
    println!("\x1b[0m\n");
    std::io::stdout().flush().ok();
}
