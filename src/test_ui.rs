use crate::render::{Renderer, Ui};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use std::error::Error;
use std::time::Duration;
use tokio::time::sleep;

pub async fn run(scene: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (ui, ui_rx) = Ui::new();
    let mut renderer = Renderer::new();

    let render_handle = tokio::spawn(async move {
        while let Ok(event) = ui_rx.recv_async().await {
            renderer.handle(event);
        }
    });

    let ui_tick = ui.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            ui_tick.tick();
        }
    });

    match scene {
        "interactive" => run_interactive(&ui).await,
        "preview" => {
            for i in 1..=3 {
                ui.set_preview(format!("Transcribing{}...", ".".repeat(i)));
                sleep(Duration::from_millis(500)).await;
            }
            ui.set_preview("Hello this is a preview".into());
            sleep(Duration::from_secs(2)).await;
        }
        "thinking" => {
            ui.set_thinking();
            sleep(Duration::from_secs(3)).await;
        }
        "speaking" => {
            ui.set_speaking();
            sleep(Duration::from_secs(3)).await;
        }
        "response" => {
            ui.set_thinking();
            sleep(Duration::from_secs(1)).await;
            for word in "Hello! I am Silly, your AI assistant.".split_whitespace() {
                ui.append_response(&format!("{} ", word));
                sleep(Duration::from_millis(100)).await;
            }
            ui.end_response();
            sleep(Duration::from_millis(500)).await;
        }
        _ => {
            println!("=== Preview ===");
            ui.set_preview("Hello this is preview text".into());
            sleep(Duration::from_secs(2)).await;

            ui.show_final("Final transcription");
            sleep(Duration::from_millis(500)).await;

            println!("=== Thinking ===");
            ui.set_thinking();
            sleep(Duration::from_secs(2)).await;

            println!("\n=== Response ===");
            for word in "Hello! I am Silly.".split_whitespace() {
                ui.append_response(&format!("{} ", word));
                sleep(Duration::from_millis(150)).await;
            }
            ui.end_response();

            println!("=== Speaking ===");
            ui.set_speaking();
            sleep(Duration::from_secs(2)).await;
        }
    }

    ui.set_idle();
    drop(ui);
    let _ = render_handle.await;
    Ok(())
}

async fn run_interactive(ui: &Ui) {
    println!("Interactive UI test. Keys:");
    println!("  p = preview, t = thinking, s = speaking");
    println!("  r = response, f = final, i = idle, q = quit\n");

    crossterm::terminal::enable_raw_mode().ok();

    loop {
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('p') => ui.set_preview("Preview text...".into()),
                    KeyCode::Char('t') => ui.set_thinking(),
                    KeyCode::Char('s') => ui.set_speaking(),
                    KeyCode::Char('r') => {
                        for word in "Streaming response text.".split_whitespace() {
                            ui.append_response(&format!("{} ", word));
                            sleep(Duration::from_millis(100)).await;
                        }
                        ui.end_response();
                    }
                    KeyCode::Char('f') => ui.show_final("Final transcription"),
                    KeyCode::Char('i') => ui.set_idle(),
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => {}
                }
            }
        }
        sleep(Duration::from_millis(10)).await;
    }

    crossterm::terminal::disable_raw_mode().ok();
}
